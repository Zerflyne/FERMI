#!/usr/bin/env python3
"""Packs a FERMI model into one GGUF file: the llama.cpp weights (LoRA already merged) plus, as metadata, the decision
head and the FERMI configuration (prompt format, evaluation). The app needs nothing else.

    python pack_fermi.py <weights.gguf> <merged folder with head.safetensors and fermi.json> <out.gguf> --name FERMI-2B

Metadata written (llama.cpp ignores keys it does not know):
    fermi.version       format version of these keys (1)
    fermi.name          "FERMI-2B"
    fermi.config        fermi.json as a JSON string (prompt_format, base_model, evaluation, training...)
    fermi.head.dim      width of the head (512)
    fermi.head.<tensor> the head's tensors as float32 arrays, row-major: a.weight [dim x n_embd], a.bias [dim],
                        b.weight [dim x n_embd], tipo.weight [3 x dim], v.weight [1 x dim], v.bias [1]
"""
import argparse
import json
import os
import sys

import numpy as np
from safetensors.numpy import load_file

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "vendor", "llama.cpp", "gguf-py"))
import gguf  # noqa: E402


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("weights")
    ap.add_argument("merged")
    ap.add_argument("out")
    ap.add_argument("--name", required=True)
    a = ap.parse_args()

    cfg = json.load(open(os.path.join(a.merged, "fermi.json"), encoding="utf-8"))
    try:
        head = load_file(os.path.join(a.merged, "head.safetensors"))
    except TypeError:                                   # bfloat16 tensors: numpy cannot hold them, go through torch
        from safetensors.torch import load_file as load_torch
        head = {k: v.float().numpy() for k, v in load_torch(os.path.join(a.merged, "head.safetensors")).items()}
    head = {k: np.ascontiguousarray(v, dtype=np.float32) for k, v in head.items()}

    reader = gguf.GGUFReader(a.weights, "r")
    arch = reader.fields[gguf.Keys.General.ARCHITECTURE].contents()
    writer = gguf.GGUFWriter(a.out, arch=arch, endianess=reader.endianess)
    al = reader.fields.get(gguf.Keys.General.ALIGNMENT)
    if al is not None:
        writer.data_alignment = al.contents()
    for field in reader.fields.values():
        if field.name == gguf.Keys.General.ARCHITECTURE or field.name.startswith("GGUF.") or field.name.startswith("fermi."):
            continue
        vt = field.types[0]
        sub = field.types[-1] if vt == gguf.GGUFValueType.ARRAY else None
        writer.add_key_value(field.name, field.contents(), vt, sub_type=sub)
    if "general.name" not in reader.fields:
        writer.add_key_value("general.name", a.name, gguf.GGUFValueType.STRING)
    writer.add_key_value("fermi.version", 1, gguf.GGUFValueType.UINT32)
    writer.add_key_value("fermi.name", a.name, gguf.GGUFValueType.STRING)
    writer.add_key_value("fermi.config", json.dumps(cfg, ensure_ascii=False), gguf.GGUFValueType.STRING)
    writer.add_key_value("fermi.head.dim", int(head["a.weight"].shape[0]), gguf.GGUFValueType.UINT32)
    for k in ("a.weight", "a.bias", "b.weight", "tipo.weight", "v.weight", "v.bias"):
        writer.add_key_value("fermi.head." + k, head[k].reshape(-1).tolist(), gguf.GGUFValueType.ARRAY,
                             sub_type=gguf.GGUFValueType.FLOAT32)
    for t in reader.tensors:
        writer.add_tensor_info(t.name, t.data.shape, t.data.dtype, t.data.nbytes, t.tensor_type)
    writer.write_header_to_file()
    writer.write_kv_data_to_file()
    writer.write_ti_data_to_file()
    for t in reader.tensors:
        writer.write_tensor_data(t.data, tensor_endianess=reader.endianess)
    writer.close()
    print("packed ->", a.out, "(%.2f GB)" % (os.path.getsize(a.out) / 1e9))


if __name__ == "__main__":
    main()
