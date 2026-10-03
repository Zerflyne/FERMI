# FERMI runtime: llama.cpp (every GPU backend of the platform as a loadable module, every CPU variant) + fermi_llama.
# Everything lands in one folder, which the app ships as it is and loads at run time.
#
#   cmake -S app/runtime -B build-runtime -DLLAMA_DIR=path/to/llama.cpp [-DFERMI_VULKAN=ON|-DFERMI_METAL=ON]
#   cmake --build build-runtime --config Release -j
cmake_minimum_required(VERSION 3.19)
project(fermi_runtime C CXX)

set(LLAMA_DIR "${CMAKE_CURRENT_SOURCE_DIR}/../../vendor/llama.cpp" CACHE PATH "llama.cpp source")
option(FERMI_VULKAN "Vulkan backend (Windows, Linux)" OFF)
option(FERMI_METAL "Metal backend (macOS)" OFF)

set(CMAKE_RUNTIME_OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/bin)
set(CMAKE_LIBRARY_OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/bin)
foreach(cfg Release RELEASE Debug DEBUG RelWithDebInfo RELWITHDEBINFO)
    set(CMAKE_RUNTIME_OUTPUT_DIRECTORY_${cfg} ${CMAKE_BINARY_DIR}/bin)
    set(CMAKE_LIBRARY_OUTPUT_DIRECTORY_${cfg} ${CMAKE_BINARY_DIR}/bin)
endforeach()
# the libraries find each other in their own folder
if (APPLE)
    set(CMAKE_INSTALL_RPATH "@loader_path")
elseif (UNIX)
    set(CMAKE_INSTALL_RPATH "$ORIGIN")
endif()
set(CMAKE_BUILD_WITH_INSTALL_RPATH ON)

set(BUILD_SHARED_LIBS ON CACHE BOOL "" FORCE)
set(GGML_BACKEND_DL ON CACHE BOOL "" FORCE)
set(GGML_NATIVE OFF CACHE BOOL "" FORCE)
if (CMAKE_SYSTEM_PROCESSOR MATCHES "x86_64|AMD64|amd64")
    set(GGML_CPU_ALL_VARIANTS ON CACHE BOOL "" FORCE)
endif()
set(GGML_VULKAN ${FERMI_VULKAN} CACHE BOOL "" FORCE)
set(GGML_METAL ${FERMI_METAL} CACHE BOOL "" FORCE)
set(GGML_METAL_EMBED_LIBRARY ON CACHE BOOL "" FORCE)
set(GGML_OPENMP OFF CACHE BOOL "" FORCE)
set(LLAMA_CURL OFF CACHE BOOL "" FORCE)
set(LLAMA_BUILD_TESTS OFF CACHE BOOL "" FORCE)
set(LLAMA_BUILD_EXAMPLES OFF CACHE BOOL "" FORCE)
set(LLAMA_BUILD_SERVER OFF CACHE BOOL "" FORCE)
set(LLAMA_BUILD_TOOLS OFF CACHE BOOL "" FORCE)
set(LLAMA_BUILD_COMMON OFF CACHE BOOL "" FORCE)

add_subdirectory(${LLAMA_DIR} llama.cpp)     # builds the libraries and every backend module

add_library(fermi_llama SHARED fermi_llama.cpp)
target_compile_features(fermi_llama PRIVATE cxx_std_17)
target_link_libraries(fermi_llama PRIVATE llama ggml)
set_target_properties(fermi_llama PROPERTIES CXX_VISIBILITY_PRESET hidden)

# plain file names (libllama.so, not libllama.so.0 -> libllama.so.0.5.0): the app ships the folder as it is, and
# installers do not always keep symbolic links
foreach(t llama ggml ggml-base)
    if (TARGET ${t})
        set_property(TARGET ${t} PROPERTY VERSION)       # no value: the property is removed
        set_property(TARGET ${t} PROPERTY SOVERSION)
    endif()
endforeach()
