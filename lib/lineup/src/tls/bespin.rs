use x86::bits64::segmentation;
use crate::tls::ThreadLocalStorage;

pub(crate) unsafe fn get_tls<'a>() -> *mut ThreadLocalStorage<'a> {
    segmentation::rdgsbase() as *mut ThreadLocalStorage
}

pub(crate) unsafe fn set_tls(t: *mut ThreadLocalStorage) {
    segmentation::wrgsbase(t as u64)
}
