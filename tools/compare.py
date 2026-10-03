#!/usr/bin/env python3
"""llama.cpp (GGUF) against PyTorch on FERMI's evaluation questions: same token ids, same head.

    python compare.py <model.gguf> <merged HF folder> <data folder> [n] [--torch] [--ngl N]
Prints agreement with the teacher for llama.cpp (and PyTorch with --torch) and how far the two are apart.
"""
import os
import subprocess
import sys
import time

import numpy as np
import torch
import torch.nn.functional as F
from safetensors.torch import load_file

HERE = os.path.dirname(os.path.abspath(__file__))


def head_probs(head, h_opt, h_end, kind):
    s = F.linear(F.gelu(F.linear(h_opt, head["a.weight"], head["a.bias"]) + F.linear(h_end, head["b.weight"])
                        + head["tipo.weight"][kind]), head["v.weight"], head["v.bias"]).squeeze(-1)
    return torch.softmax(s, -1)


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    gguf, merged, data = args[:3]
    n = int(args[3]) if len(args) > 3 else 200
    ngl = sys.argv[sys.argv.index("--ngl") + 1] if "--ngl" in sys.argv else "999"
    if "--ngl" in sys.argv:
        args = [a for a in args if a != ngl]
    z = np.load(os.path.join(data, "esempi.npz"))
    tok = np.memmap(os.path.join(data, "token.bin"), dtype=np.uint32, mode="r")
    starts_pos = np.concatenate([[0], np.cumsum(z["npos"])])
    ev = np.flatnonzero(z["parte"] == 1)
    rng = np.random.default_rng(0)
    pick = np.sort(rng.choice(ev, size=min(n, len(ev)), replace=False))
    ex = []
    for i in pick:
        ids = tok[z["inizio"][i]: z["inizio"][i] + z["lung"][i]].astype(np.int64)
        pos = z["pos"][starts_pos[i]: starts_pos[i + 1]]
        tgt = z["obiettivo"][starts_pos[i]: starts_pos[i + 1]]
        ex.append((ids, pos, tgt, int(z["tipo"][i])))
    tmp = os.path.join(HERE, ".cmp")
    os.makedirs(tmp, exist_ok=True)
    with open(os.path.join(tmp, "in.txt"), "w") as f:
        for ids, pos, _, _ in ex:
            f.write(" ".join(map(str, ids)) + " | " + " ".join(map(str, pos)) + "\n")
    t = time.time()
    r = subprocess.run([os.path.join(HERE, "probe", "probe"), gguf, os.path.join(tmp, "in.txt"), os.path.join(tmp, "out.bin"), ngl],
                       capture_output=True, text=True)
    print(r.stderr.strip().splitlines()[-1], "| wall %.1f s" % (time.time() - t))
    head = {k: v.float() for k, v in load_file(os.path.join(merged, "head.safetensors")).items()}
    d = head["a.weight"].shape[1]
    raw = np.fromfile(os.path.join(tmp, "out.bin"), dtype=np.float32).reshape(-1, d)
    ref = None
    if "--torch" in sys.argv:
        from transformers import AutoModelForCausalLM
        sys.path.insert(0, os.path.join(HERE, "..", "..", "archivio", "engine-python"))
        from fermi.model import _kernels
        if "qwen3_5" in open(os.path.join(merged, "config.json")).read():
            _kernels(torch.device("cpu"))         # the PyTorch reference kernels, not the Triton ones
        full = AutoModelForCausalLM.from_pretrained(merged, dtype=getattr(torch, os.environ.get("REF_DTYPE", "float32")))
        model = full.model if hasattr(full, "model") and hasattr(full.model, "layers") else full.get_decoder()
        model.eval()
        ref = []
    k = 0
    agree = agree_t = same = 0
    ce = ce_t = kl = 0.0
    for j, (ids, pos, tgt, kind) in enumerate(ex):
        h = torch.from_numpy(raw[k: k + len(pos) + 1]); k += len(pos) + 1
        p = head_probs(head, h[:-1], h[-1].expand(len(pos), -1), torch.full((len(pos),), kind))
        tgt = torch.from_numpy(tgt)
        agree += int(p.argmax() == tgt.argmax()); ce += float(-(tgt * p.clamp_min(1e-9).log()).sum())
        if ref is not None:
            with torch.no_grad():
                hh = model(input_ids=torch.from_numpy(ids)[None]).last_hidden_state[0]
            hh = hh.float()
            q = head_probs(head, hh[torch.from_numpy(pos)], hh[-1].expand(len(pos), -1), torch.full((len(pos),), kind))
            agree_t += int(q.argmax() == tgt.argmax()); ce_t += float(-(tgt * q.clamp_min(1e-9).log()).sum())
            same += int(q.argmax() == p.argmax()); kl += float((q * (q.clamp_min(1e-9).log() - p.clamp_min(1e-9).log())).sum())
            if j % 25 == 0:
                print("  %d/%d  max|p-q| %.4f" % (j, len(ex), float((p - q).abs().max())), flush=True)
    m = len(ex)
    print("llama.cpp: agreement with the teacher %.1f%%, cross-entropy %.3f" % (100 * agree / m, ce / m))
    if ref is not None:
        print("PyTorch  : agreement with the teacher %.1f%%, cross-entropy %.3f" % (100 * agree_t / m, ce_t / m))
        print("same answer %.1f%%, KL(PyTorch || llama.cpp) %.4f" % (100 * same / m, kl / m))


if __name__ == "__main__":
    main()
