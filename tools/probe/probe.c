// FERMI probe: runs token ids through llama.cpp and writes the final hidden states at chosen positions.
// input (text): one sequence per line, "id id id ... | pos pos ...". output (binary float32): for every
// sequence, the hidden state of every listed position, then of the last token.
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include "llama.h"

static double now(void) { struct timespec t; clock_gettime(CLOCK_MONOTONIC, &t); return t.tv_sec + t.tv_nsec * 1e-9; }

int main(int argc, char **argv) {
    if (argc < 4) { fprintf(stderr, "usage: probe model.gguf in.txt out.bin [n_gpu_layers]\n"); return 1; }
    int ngl = argc > 4 ? atoi(argv[4]) : 999;
    if (getenv("FERMI_BACKENDS")) ggml_backend_load_all_from_path(getenv("FERMI_BACKENDS")); else ggml_backend_load_all();
    llama_backend_init();
    struct llama_model_params mp = llama_model_default_params();
    mp.n_gpu_layers = ngl;
    struct llama_model *model = llama_model_load_from_file(argv[1], mp);
    if (!model) { fprintf(stderr, "cannot load model\n"); return 1; }
    struct llama_context_params cp = llama_context_default_params();
    cp.n_ctx = 4096; cp.n_batch = 4096; cp.n_ubatch = getenv("FERMI_UBATCH") ? atoi(getenv("FERMI_UBATCH")) : 512; cp.n_seq_max = 1;
    cp.embeddings = true; cp.pooling_type = LLAMA_POOLING_TYPE_NONE;
    struct llama_context *ctx = llama_init_from_model(model, cp);
    if (!ctx) { fprintf(stderr, "cannot create context\n"); return 1; }
    int d = llama_model_n_embd(model);
    FILE *in = fopen(argv[2], "r"), *out = fopen(argv[3], "wb");
    char *line = NULL; size_t cap = 0;
    int *ids = malloc(sizeof(int) * 8192), *pos = malloc(sizeof(int) * 64);
    double total = 0; int nseq = 0;
    while (getline(&line, &cap, in) > 0) {
        int n = 0, np = 0, side = 0;
        for (char *tok = strtok(line, " \n"); tok; tok = strtok(NULL, " \n")) {
            if (!strcmp(tok, "|")) { side = 1; continue; }
            if (side) pos[np++] = atoi(tok); else ids[n++] = atoi(tok);
        }
        pos[np++] = n - 1;
        struct llama_batch b = llama_batch_init(n, 0, 1);
        for (int i = 0; i < n; i++) { b.token[i] = ids[i]; b.pos[i] = i; b.n_seq_id[i] = 1; b.seq_id[i][0] = 0; b.logits[i] = 0; }
        for (int j = 0; j < np; j++) b.logits[pos[j]] = 1;
        b.n_tokens = n;
        llama_memory_clear(llama_get_memory(ctx), true);
        double t = now();
        if (llama_decode(ctx, b)) { fprintf(stderr, "decode failed\n"); return 1; }
        for (int j = 0; j < np; j++) fwrite(llama_get_embeddings_ith(ctx, pos[j]), sizeof(float), d, out);
        total += now() - t; nseq++;
        llama_batch_free(b);
    }
    fprintf(stderr, "sequences %d, dim %d, mean %.1f ms\n", nseq, d, 1000 * total / nseq);
    fclose(out);
    llama_free(ctx); llama_model_free(model);
    return 0;
}
