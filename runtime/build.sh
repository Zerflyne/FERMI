#!/usr/bin/env bash
# Builds the FERMI runtime (llama.cpp + fermi_llama) into app/runtime/dist, which the app bundles as "runtime/".
# Linux and Windows (MSYS/Git Bash): Vulkan backend + every CPU variant. macOS: Metal + CPU.
#
#   app/runtime/build.sh [path/to/llama.cpp]        (default: vendor/llama.cpp next to app/)
# Needs cmake, a C++ compiler and, except on macOS, the Vulkan SDK (glslc, Vulkan headers, SPIRV-Headers).
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
LLAMA="${1:-$HERE/../../vendor/llama.cpp}"
BUILD="${FERMI_RUNTIME_BUILD:-$HERE/build}"
DIST="$HERE/dist"

# FERMI's changes to llama.cpp (patches/*.patch), applied once
for p in "$HERE"/patches/*.patch; do
  if git -C "$LLAMA" apply --check "$p" 2>/dev/null; then
    git -C "$LLAMA" apply "$p" && echo "applied $(basename "$p")"
  elif git -C "$LLAMA" apply --reverse --check "$p" 2>/dev/null; then
    echo "already applied: $(basename "$p")"
  else
    echo "cannot apply $(basename "$p") to $LLAMA (llama.cpp version: see LLAMA_COMMIT)" >&2; exit 1
  fi
done

case "$(uname -s)" in
  Darwin) GPU=(-DFERMI_METAL=ON) ;;
  *)      GPU=(-DFERMI_VULKAN=ON) ;;
esac

cmake -S "$HERE" -B "$BUILD" -DLLAMA_DIR="$LLAMA" -DCMAKE_BUILD_TYPE=Release "${GPU[@]}" ${FERMI_CMAKE_ARGS:-}
cmake --build "$BUILD" --config Release -j "${JOBS:-$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 4)}"

rm -rf "$DIST"
mkdir -p "$DIST"
# the libraries and the backend modules, without the versioned symlink copies
find "$BUILD/bin" -maxdepth 1 -type f \( -name "*.so" -o -name "*.so.*" -o -name "*.dylib" -o -name "*.dll" \) -exec cp -P {} "$DIST/" \;
find "$BUILD/bin" -maxdepth 1 -type l \( -name "*.so*" -o -name "*.dylib" \) -exec cp -P {} "$DIST/" \;
echo "runtime in $DIST:"
ls -la "$DIST"
