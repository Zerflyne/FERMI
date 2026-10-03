// fermi_llama: the small C layer between the FERMI app (Rust) and llama.cpp.
// Plain types only, so that the app can load it at run time (dlopen / LoadLibrary) on every platform.
#pragma once
#include <stddef.h>
#include <stdint.h>

#ifdef _WIN32
#  define FL_API __declspec(dllexport)
#else
#  define FL_API __attribute__((visibility("default")))
#endif

#ifdef __cplusplus
extern "C" {
#endif

typedef struct fl_model fl_model;

// loads the compute backends (Vulkan, Metal, CPU variants...) found in dir; call once. Returns the number of devices.
FL_API int fl_init(const char * backend_dir);
FL_API int fl_device_count(void);
// type: 0 CPU, 1 GPU, 2 integrated GPU, 3 other. Strings are copied into the given buffers.
FL_API int fl_device_info(int i, char * name, int name_len, char * desc, int desc_len, size_t * free_mem, size_t * total_mem, int * type);

// device: index from fl_device_info (a GPU), or -1 for the CPU only. n_ctx: tokens held at once (all sequences together);
// n_seq: sequences per call; n_threads: CPU threads, 0 = automatic.
FL_API fl_model * fl_load(const char * path, int device, int n_ctx, int n_seq, int n_ubatch, int n_threads, char * err, int err_len);
FL_API void fl_free(fl_model * m);
FL_API int fl_n_embd(const fl_model * m);
FL_API int fl_n_ctx(const fl_model * m);
FL_API int fl_n_seq(const fl_model * m);

// text -> token ids, special tokens written as text are parsed (<|im_start|>...), nothing is added. Returns the count,
// or -needed when max is too small.
FL_API int fl_tokenize(const fl_model * m, const char * text, int text_len, int32_t * out, int max);

// n_seq sequences in one pass. tokens: all sequences one after the other; seq_len[i]: length of sequence i;
// npos[i]: how many hidden states are wanted from sequence i; pos: their positions, relative to the start of each
// sequence, all sequences one after the other. out: sum(npos) x n_embd floats, in the same order. Returns 0 on success.
FL_API int fl_hidden(fl_model * m, int n_seq, const int32_t * tokens, const int32_t * seq_len, const int32_t * npos,
                     const int32_t * pos, float * out, char * err, int err_len);

// metadata of a GGUF file, read without loading the weights (the FERMI head and configuration live here).
typedef struct fl_meta fl_meta;
FL_API fl_meta * fl_meta_open(const char * path);
FL_API void fl_meta_close(fl_meta * m);
// string value: returns its length in bytes (copied up to max-1, NUL-terminated), -1 when the key is missing
FL_API int fl_meta_str(const fl_meta * m, const char * key, char * out, int max);
// integer value (any integer type), def when the key is missing
FL_API int64_t fl_meta_int(const fl_meta * m, const char * key, int64_t def);
// float32 array: returns its length (copies up to max values), -1 when the key is missing or not a float array
FL_API int64_t fl_meta_f32(const fl_meta * m, const char * key, float * out, int64_t max);

#ifdef __cplusplus
}
#endif
