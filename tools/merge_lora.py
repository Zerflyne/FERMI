#!/usr/bin/env python3
"""Merges a FERMI checkpoint's LoRA adapter into its base model and saves a plain Hugging Face model, ready for
llama.cpp's convert_hf_to_gguf.py. The decision head is copied next to it (head.safetensors + fermi.json).

    python merge_lora.py <checkpoint folder> <base folder or HF id> <output folder>
"""
import json
import os
import shutil
import sys

import torch
from peft import LoraConfig, get_peft_model, set_peft_model_state_dict
from safetensors.torch import load_file
from transformers import AutoModelForCausalLM, AutoTokenizer


def main(ckpt, base, out):
    cfg = json.load(open(os.path.join(ckpt, "config.json")))
    tok = AutoTokenizer.from_pretrained(base)
    full = AutoModelForCausalLM.from_pretrained(base, dtype=torch.bfloat16)
    text = full.model if hasattr(full, "model") and hasattr(full.model, "layers") else full.get_decoder()
    lcfg = LoraConfig(r=cfg["lora_rank"], lora_alpha=cfg["lora_alpha"], target_modules=cfg["lora_targets"],
                      lora_dropout=0.0, bias="none")
    peft = get_peft_model(text, lcfg)
    adapter = {k: v.to(torch.float32) for k, v in load_file(os.path.join(ckpt, "adapter.safetensors")).items()}
    res = set_peft_model_state_dict(peft, adapter)
    if getattr(res, "unexpected_keys", None):
        raise SystemExit("adapter keys not used: %s" % res.unexpected_keys[:5])
    merged = peft.merge_and_unload()
    if full.model is not merged and hasattr(full, "model"):
        full.model = merged
    os.makedirs(out, exist_ok=True)
    full.config.mtp_num_hidden_layers = 0      # the multi-token-prediction block is not saved and FERMI does not use it
    full.save_pretrained(out, safe_serialization=True)
    tok.save_pretrained(out)
    shutil.copy(os.path.join(ckpt, "head.safetensors"), os.path.join(out, "head.safetensors"))
    json.dump(cfg, open(os.path.join(out, "fermi.json"), "w"), indent=1)
    print("merged ->", out)


if __name__ == "__main__":
    main(*sys.argv[1:4])
