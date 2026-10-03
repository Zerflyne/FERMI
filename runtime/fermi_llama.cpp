// fermi_llama: see fermi_llama.h.
#include "fermi_llama.h"

#include <cstdio>
#include <cstring>
#include <mutex>
#include <string>
#include <vector>

#include "ggml-backend.h"
#include "gguf.h"
#include "llama.h"

struct fl_model {
    llama_model * model = nullptr;
    llama_context * ctx = nullptr;
    const llama_vocab * vocab = nullptr;
    int n_embd = 0, n_ctx = 0, n_seq = 0;
    std::mutex lock;
};

static void set_err(char * err, int len, const std::string & msg) {
    if (err && len > 0) {
        snprintf(err, (size_t) len, "%s", msg.c_str());
    }
}

static std::once_flag g_once;
static int g_devices = 0;

static void quiet_log(ggml_log_level level, const char * text, void *) {
    if (level >= GGML_LOG_LEVEL_WARN) {
        fputs(text, stderr);
    }
}

int fl_init(const char * backend_dir) {
    std::call_once(g_once, [&] {
        llama_log_set(quiet_log, nullptr);
        if (backend_dir && *backend_dir) {
            ggml_backend_load_all_from_path(backend_dir);
        } else {
            ggml_backend_load_all();
        }
        llama_backend_init();
        g_devices = (int) ggml_backend_dev_count();
    });
    return g_devices;
}

int fl_device_count(void) { return (int) ggml_backend_dev_count(); }

int fl_device_info(int i, char * name, int name_len, char * desc, int desc_len, size_t * free_mem, size_t * total_mem, int * type) {
    if (i < 0 || i >= (int) ggml_backend_dev_count()) {
        return -1;
    }
    ggml_backend_dev_t d = ggml_backend_dev_get(i);
    if (name && name_len > 0) snprintf(name, (size_t) name_len, "%s", ggml_backend_dev_name(d));
    if (desc && desc_len > 0) snprintf(desc, (size_t) desc_len, "%s", ggml_backend_dev_description(d));
    size_t f = 0, t = 0;
    ggml_backend_dev_memory(d, &f, &t);
    if (free_mem) *free_mem = f;
    if (total_mem) *total_mem = t;
    if (type) {
        switch (ggml_backend_dev_type(d)) {
            case GGML_BACKEND_DEVICE_TYPE_CPU:  *type = 0; break;
            case GGML_BACKEND_DEVICE_TYPE_GPU:  *type = 1; break;
            case GGML_BACKEND_DEVICE_TYPE_IGPU: *type = 2; break;
            default:                            *type = 3; break;
        }
    }
    return 0;
}

fl_model * fl_load(const char * path, int device, int n_ctx, int n_seq, int n_ubatch, int n_threads, char * err, int err_len) {
    llama_model_params mp = llama_model_default_params();
    ggml_backend_dev_t devs[2] = {nullptr, nullptr};
    if (device >= 0) {
        if (device >= (int) ggml_backend_dev_count()) {
            set_err(err, err_len, "device not found");
            return nullptr;
        }
        devs[0] = ggml_backend_dev_get(device);
        mp.devices = devs;                     // this device only: never spread over every GPU of the computer
        mp.n_gpu_layers = 999;
        mp.split_mode = LLAMA_SPLIT_MODE_NONE;
    } else {
        mp.devices = devs;                     // empty list: no GPU
        mp.n_gpu_layers = 0;
    }
    llama_model * model = llama_model_load_from_file(path, mp);
    if (!model) {
        set_err(err, err_len, "cannot load the model file");
        return nullptr;
    }
    llama_context_params cp = llama_context_default_params();
    cp.n_ctx = (uint32_t) n_ctx;
    cp.n_batch = (uint32_t) n_ctx;             // a whole call in one llama_decode
    cp.n_ubatch = (uint32_t) (n_ubatch > 0 ? n_ubatch : 512);
    cp.n_seq_max = (uint32_t) n_seq;
    cp.n_outputs_max = (uint32_t) n_seq * 32;  // the option lines and the last token of each sequence: keeps the
                                               // output buffers small (needs patches/0001: no forced output of all)
    cp.kv_unified = true;
    cp.embeddings = true;
    cp.pooling_type = LLAMA_POOLING_TYPE_NONE;
    cp.no_perf = true;
    if (n_threads > 0) {
        cp.n_threads = n_threads;
        cp.n_threads_batch = n_threads;
    }
    if (device < 0) {
        cp.op_offload = false;                 // CPU only means CPU only
    }
    llama_context * ctx = llama_init_from_model(model, cp);
    if (!ctx) {
        llama_model_free(model);
        set_err(err, err_len, "not enough memory for the model's working buffers");
        return nullptr;
    }
    fl_model * m = new fl_model;
    m->model = model;
    m->ctx = ctx;
    m->vocab = llama_model_get_vocab(model);
    m->n_embd = llama_model_n_embd(model);
    m->n_ctx = (int) llama_n_ctx(ctx);
    m->n_seq = n_seq;
    return m;
}

void fl_free(fl_model * m) {
    if (!m) return;
    llama_free(m->ctx);
    llama_model_free(m->model);
    delete m;
}

int fl_n_embd(const fl_model * m) { return m->n_embd; }
int fl_n_ctx(const fl_model * m) { return m->n_ctx; }
int fl_n_seq(const fl_model * m) { return m->n_seq; }

int fl_tokenize(const fl_model * m, const char * text, int text_len, int32_t * out, int max) {
    return llama_tokenize(m->vocab, text, text_len, out, max, /*add_special*/ false, /*parse_special*/ true);
}

int fl_hidden(fl_model * m, int n_seq, const int32_t * tokens, const int32_t * seq_len, const int32_t * npos,
              const int32_t * pos, float * out, char * err, int err_len) {
    std::lock_guard<std::mutex> g(m->lock);
    if (n_seq < 1 || n_seq > m->n_seq) {
        set_err(err, err_len, "too many sequences in one call");
        return -1;
    }
    int total = 0;
    for (int s = 0; s < n_seq; s++) total += seq_len[s];
    if (total > m->n_ctx) {
        set_err(err, err_len, "too many tokens in one call");
        return -1;
    }
    llama_batch b = llama_batch_init(total, 0, 1);
    std::vector<int> want;                     // batch index of every wanted hidden state, in output order
    int k = 0, p = 0;
    for (int s = 0; s < n_seq; s++) {
        for (int i = 0; i < seq_len[s]; i++, k++) {
            b.token[k] = tokens[k];
            b.pos[k] = i;
            b.n_seq_id[k] = 1;
            b.seq_id[k][0] = s;
            b.logits[k] = 0;
        }
        for (int j = 0; j < npos[s]; j++, p++) {
            int at = k - seq_len[s] + pos[p];
            if (pos[p] < 0 || pos[p] >= seq_len[s]) {
                llama_batch_free(b);
                set_err(err, err_len, "position outside its sequence");
                return -1;
            }
            b.logits[at] = 1;
            want.push_back(at);
        }
    }
    b.n_tokens = total;
    llama_memory_clear(llama_get_memory(m->ctx), true);
    int rc = llama_decode(m->ctx, b);
    if (rc != 0) {
        llama_batch_free(b);
        set_err(err, err_len, "llama_decode failed (" + std::to_string(rc) + ")");
        return -1;
    }
    for (size_t j = 0; j < want.size(); j++) {
        const float * h = llama_get_embeddings_ith(m->ctx, want[j]);
        if (!h) {
            llama_batch_free(b);
            set_err(err, err_len, "hidden state not available");
            return -1;
        }
        memcpy(out + j * (size_t) m->n_embd, h, sizeof(float) * (size_t) m->n_embd);
    }
    llama_batch_free(b);
    return 0;
}

// ------------------------------------------------------------------------------------------------------ metadata
struct fl_meta {
    gguf_context * ctx = nullptr;
};

fl_meta * fl_meta_open(const char * path) {
    gguf_init_params p = {/*no_alloc*/ true, /*ctx*/ nullptr};
    gguf_context * ctx = gguf_init_from_file(path, p);
    if (!ctx) return nullptr;
    fl_meta * m = new fl_meta;
    m->ctx = ctx;
    return m;
}

void fl_meta_close(fl_meta * m) {
    if (!m) return;
    gguf_free(m->ctx);
    delete m;
}

int fl_meta_str(const fl_meta * m, const char * key, char * out, int max) {
    int64_t k = gguf_find_key(m->ctx, key);
    if (k < 0 || gguf_get_kv_type(m->ctx, k) != GGUF_TYPE_STRING) return -1;
    const char * v = gguf_get_val_str(m->ctx, k);
    int n = (int) strlen(v);
    if (out && max > 0) {
        int c = n < max - 1 ? n : max - 1;
        memcpy(out, v, (size_t) c);
        out[c] = 0;
    }
    return n;
}

int64_t fl_meta_int(const fl_meta * m, const char * key, int64_t def) {
    int64_t k = gguf_find_key(m->ctx, key);
    if (k < 0) return def;
    switch (gguf_get_kv_type(m->ctx, k)) {
        case GGUF_TYPE_UINT8:  return gguf_get_val_u8(m->ctx, k);
        case GGUF_TYPE_INT8:   return gguf_get_val_i8(m->ctx, k);
        case GGUF_TYPE_UINT16: return gguf_get_val_u16(m->ctx, k);
        case GGUF_TYPE_INT16:  return gguf_get_val_i16(m->ctx, k);
        case GGUF_TYPE_UINT32: return gguf_get_val_u32(m->ctx, k);
        case GGUF_TYPE_INT32:  return gguf_get_val_i32(m->ctx, k);
        case GGUF_TYPE_UINT64: return (int64_t) gguf_get_val_u64(m->ctx, k);
        case GGUF_TYPE_INT64:  return gguf_get_val_i64(m->ctx, k);
        default:               return def;
    }
}

int64_t fl_meta_f32(const fl_meta * m, const char * key, float * out, int64_t max) {
    int64_t k = gguf_find_key(m->ctx, key);
    if (k < 0 || gguf_get_kv_type(m->ctx, k) != GGUF_TYPE_ARRAY || gguf_get_arr_type(m->ctx, k) != GGUF_TYPE_FLOAT32) return -1;
    int64_t n = (int64_t) gguf_get_arr_n(m->ctx, k);
    if (out && max > 0) {
        memcpy(out, gguf_get_arr_data(m->ctx, k), sizeof(float) * (size_t) (n < max ? n : max));
    }
    return n;
}
