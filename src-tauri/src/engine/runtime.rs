//! The llama.cpp runtime: `fermi_llama` (app/runtime) and the llama.cpp libraries and backend modules next to it,
//! loaded at run time from one folder. The app starts even when the folder is missing or a GPU driver is not there:
//! the backends that cannot load are skipped and the CPU is always available.

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::path::{Path, PathBuf};

use libloading::Library;
use serde::Serialize;

type FnInit = unsafe extern "C" fn(*const c_char) -> c_int;
type FnCount = unsafe extern "C" fn() -> c_int;
type FnDevInfo =
    unsafe extern "C" fn(c_int, *mut c_char, c_int, *mut c_char, c_int, *mut usize, *mut usize, *mut c_int) -> c_int;
type FnLoad = unsafe extern "C" fn(*const c_char, c_int, c_int, c_int, c_int, c_int, *mut c_char, c_int) -> *mut c_void;
type FnFree = unsafe extern "C" fn(*mut c_void);
type FnInt = unsafe extern "C" fn(*const c_void) -> c_int;
type FnTok = unsafe extern "C" fn(*const c_void, *const c_char, c_int, *mut i32, c_int) -> c_int;
type FnHidden = unsafe extern "C" fn(
    *mut c_void, c_int, *const i32, *const i32, *const i32, *const i32, *mut f32, *mut c_char, c_int,
) -> c_int;
type FnMetaOpen = unsafe extern "C" fn(*const c_char) -> *mut c_void;
type FnMetaStr = unsafe extern "C" fn(*const c_void, *const c_char, *mut c_char, c_int) -> c_int;
type FnMetaInt = unsafe extern "C" fn(*const c_void, *const c_char, i64) -> i64;
type FnMetaF32 = unsafe extern "C" fn(*const c_void, *const c_char, *mut f32, i64) -> i64;

#[derive(Clone, Copy)]
struct Fns {
    device_count: FnCount,
    device_info: FnDevInfo,
    load: FnLoad,
    free: FnFree,
    n_embd: FnInt,
    n_ctx: FnInt,
    n_seq: FnInt,
    tokenize: FnTok,
    hidden: FnHidden,
    meta_open: FnMetaOpen,
    meta_close: FnFree,
    meta_str: FnMetaStr,
    meta_int: FnMetaInt,
    meta_f32: FnMetaF32,
}

/// A compute device seen by llama.cpp: a GPU (Vulkan, Metal...) or the CPU.
#[derive(Clone, Debug, Serialize)]
pub struct Device {
    pub index: i32,
    /// "Vulkan0", "Metal", "CPU"...
    pub name: String,
    pub description: String,
    /// cpu, gpu, igpu, other
    pub kind: &'static str,
    pub free: u64,
    pub total: u64,
}

pub struct Runtime {
    _lib: Library,
    f: Fns,
    pub dir: PathBuf,
}

// the C functions are thread-safe where the engine calls them from several threads (see fermi_llama.cpp)
unsafe impl Send for Runtime {}
unsafe impl Sync for Runtime {}

pub fn lib_name() -> &'static str {
    if cfg!(windows) {
        "fermi_llama.dll"
    } else if cfg!(target_os = "macos") {
        "libfermi_llama.dylib"
    } else {
        "libfermi_llama.so"
    }
}

fn err_text(buf: &[u8]) -> String {
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..end]).into_owned()
}

impl Runtime {
    pub fn load(dir: &Path) -> Result<Runtime, String> {
        let path = dir.join(lib_name());
        if !path.is_file() {
            return Err(format!("runtime not found: {}", path.display()));
        }
        #[cfg(windows)]
        let lib = {
            // the DLL's own folder first: llama.dll and ggml*.dll sit next to it
            use libloading::os::windows::{Library as WinLib, LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR};
            unsafe { WinLib::load_with_flags(&path, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS) }
                .map(Library::from)
        };
        #[cfg(not(windows))]
        let lib = unsafe { Library::new(&path) };
        let lib = lib.map_err(|e| format!("cannot load {}: {e}", path.display()))?;
        macro_rules! sym {
            ($name:literal) => {
                *unsafe { lib.get(concat!($name, "\0").as_bytes()) }.map_err(|e| format!("{}: {e}", $name))?
            };
        }
        let init: FnInit = sym!("fl_init");
        let f = Fns {
            device_count: sym!("fl_device_count"),
            device_info: sym!("fl_device_info"),
            load: sym!("fl_load"),
            free: sym!("fl_free"),
            n_embd: sym!("fl_n_embd"),
            n_ctx: sym!("fl_n_ctx"),
            n_seq: sym!("fl_n_seq"),
            tokenize: sym!("fl_tokenize"),
            hidden: sym!("fl_hidden"),
            meta_open: sym!("fl_meta_open"),
            meta_close: sym!("fl_meta_close"),
            meta_str: sym!("fl_meta_str"),
            meta_int: sym!("fl_meta_int"),
            meta_f32: sym!("fl_meta_f32"),
        };
        let cdir = CString::new(dir.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
        unsafe { init(cdir.as_ptr()) };
        Ok(Runtime { _lib: lib, f, dir: dir.to_path_buf() })
    }

    /// The devices, with their memory as it is now.
    pub fn devices(&self) -> Vec<Device> {
        let n = unsafe { (self.f.device_count)() };
        let mut out = Vec::new();
        for i in 0..n {
            let mut name = [0u8; 128];
            let mut desc = [0u8; 256];
            let (mut free, mut total, mut kind) = (0usize, 0usize, 3 as c_int);
            let rc = unsafe {
                (self.f.device_info)(i, name.as_mut_ptr() as *mut c_char, 128, desc.as_mut_ptr() as *mut c_char, 256,
                                     &mut free, &mut total, &mut kind)
            };
            if rc != 0 {
                continue;
            }
            out.push(Device {
                index: i,
                name: err_text(&name),
                description: err_text(&desc),
                kind: match kind { 0 => "cpu", 1 => "gpu", 2 => "igpu", _ => "other" },
                free: free as u64,
                total: total as u64,
            });
        }
        out
    }

    /// device: a device index (a GPU), or -1 for the CPU only.
    pub fn load_model(&self, path: &Path, device: i32, n_ctx: i32, n_seq: i32, n_ubatch: i32, threads: i32) -> Result<LlamaModel, String> {
        let c = CString::new(path.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
        let mut err = [0u8; 512];
        let ptr = unsafe { (self.f.load)(c.as_ptr(), device, n_ctx, n_seq, n_ubatch, threads, err.as_mut_ptr() as *mut c_char, 512) };
        if ptr.is_null() {
            return Err(err_text(&err));
        }
        let n_embd = unsafe { (self.f.n_embd)(ptr) } as usize;
        let n_ctx = unsafe { (self.f.n_ctx)(ptr) } as usize;
        let n_seq = unsafe { (self.f.n_seq)(ptr) } as usize;
        Ok(LlamaModel { f: self.f, ptr, n_embd, n_ctx, n_seq })
    }

    pub fn meta(&self, path: &Path) -> Option<Meta> {
        let c = CString::new(path.to_string_lossy().as_bytes()).ok()?;
        let ptr = unsafe { (self.f.meta_open)(c.as_ptr()) };
        (!ptr.is_null()).then_some(Meta { f: self.f, ptr })
    }
}

/// A model loaded by llama.cpp, with a context that returns hidden states.
pub struct LlamaModel {
    f: Fns,
    ptr: *mut c_void,
    pub n_embd: usize,
    pub n_ctx: usize,
    pub n_seq: usize,
}

// fl_hidden takes the model's own lock; tokenizing only reads the vocabulary
unsafe impl Send for LlamaModel {}
unsafe impl Sync for LlamaModel {}

impl Drop for LlamaModel {
    fn drop(&mut self) {
        unsafe { (self.f.free)(self.ptr) };
    }
}

impl LlamaModel {
    /// Text -> token ids; special tokens written in the text (<|im_start|>...) are parsed, nothing is added.
    pub fn tokenize(&self, text: &str) -> Vec<i32> {
        let mut out = vec![0i32; text.len() + 16];
        loop {
            let n = unsafe {
                (self.f.tokenize)(self.ptr, text.as_ptr() as *const c_char, text.len() as c_int, out.as_mut_ptr(), out.len() as c_int)
            };
            if n >= 0 {
                out.truncate(n as usize);
                return out;
            }
            out.resize((-n) as usize + 16, 0);
        }
    }

    /// Hidden states of several sequences in one pass: seqs = [(tokens, wanted positions)]. Returns, in order, the
    /// n_embd floats of every wanted position of every sequence.
    pub fn hidden(&self, seqs: &[(&[i32], &[i32])]) -> Result<Vec<f32>, String> {
        let mut tokens = Vec::new();
        let mut lens = Vec::new();
        let mut npos = Vec::new();
        let mut pos = Vec::new();
        for (t, p) in seqs {
            tokens.extend_from_slice(t);
            lens.push(t.len() as i32);
            npos.push(p.len() as i32);
            pos.extend_from_slice(p);
        }
        let mut out = vec![0f32; pos.len() * self.n_embd];
        let mut err = [0u8; 512];
        let rc = unsafe {
            (self.f.hidden)(self.ptr, seqs.len() as c_int, tokens.as_ptr(), lens.as_ptr(), npos.as_ptr(), pos.as_ptr(),
                            out.as_mut_ptr(), err.as_mut_ptr() as *mut c_char, 512)
        };
        if rc != 0 {
            return Err(err_text(&err));
        }
        Ok(out)
    }
}

/// Metadata of a GGUF file (no weights loaded).
pub struct Meta {
    f: Fns,
    ptr: *mut c_void,
}

impl Drop for Meta {
    fn drop(&mut self) {
        unsafe { (self.f.meta_close)(self.ptr) };
    }
}

impl Meta {
    pub fn str(&self, key: &str) -> Option<String> {
        let k = CString::new(key).ok()?;
        let n = unsafe { (self.f.meta_str)(self.ptr, k.as_ptr(), std::ptr::null_mut(), 0) };
        if n < 0 {
            return None;
        }
        let mut buf = vec![0u8; n as usize + 1];
        unsafe { (self.f.meta_str)(self.ptr, k.as_ptr(), buf.as_mut_ptr() as *mut c_char, buf.len() as c_int) };
        Some(unsafe { CStr::from_ptr(buf.as_ptr() as *const c_char) }.to_string_lossy().into_owned())
    }

    pub fn int(&self, key: &str) -> Option<i64> {
        let k = CString::new(key).ok()?;
        let v = unsafe { (self.f.meta_int)(self.ptr, k.as_ptr(), i64::MIN) };
        (v != i64::MIN).then_some(v)
    }

    pub fn f32s(&self, key: &str) -> Option<Vec<f32>> {
        let k = CString::new(key).ok()?;
        let n = unsafe { (self.f.meta_f32)(self.ptr, k.as_ptr(), std::ptr::null_mut(), 0) };
        if n < 0 {
            return None;
        }
        let mut v = vec![0f32; n as usize];
        unsafe { (self.f.meta_f32)(self.ptr, k.as_ptr(), v.as_mut_ptr(), n) };
        Some(v)
    }
}
