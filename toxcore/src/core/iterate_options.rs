use crate::ffi;
use crate::types::Tox_Err_Iterate_Options_New;

pub struct IterateOptions {
    pub(crate) ptr: *mut ffi::Tox_Iterate_Options,
}

unsafe impl Send for IterateOptions {}
unsafe impl Sync for IterateOptions {}

impl Drop for IterateOptions {
    fn drop(&mut self) {
        unsafe { ffi::tox_iterate_options_free(self.ptr) }
    }
}

impl Default for IterateOptions {
    fn default() -> Self {
        Self::new().expect("Failed to create default IterateOptions")
    }
}

impl IterateOptions {
    pub fn new() -> Result<Self, Tox_Err_Iterate_Options_New> {
        let ptr = ffi_call!(
            tox_iterate_options_new,
            ffi::Tox_Err_Iterate_Options_New::TOX_ERR_ITERATE_OPTIONS_NEW_OK
        )
        .map_err(Tox_Err_Iterate_Options_New::from)?;
        Ok(Self { ptr })
    }

    pub fn fail_hard(&mut self, fail_hard: bool) {
        unsafe { ffi::tox_iterate_options_set_fail_hard(self.ptr, fail_hard) };
    }

    pub fn get_fail_hard(&self) -> bool {
        unsafe { ffi::tox_iterate_options_get_fail_hard(self.ptr) }
    }
}
