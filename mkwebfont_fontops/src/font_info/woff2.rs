//! Code from <https://github.com/bodoni/woff/tree/main>
//! Vendored to avoid extremely-low usage dependency and so I can later deal with the code quality

use anyhow::*;

extern crate brotli; // force include

extern "C" {
    fn ComputeTTFToWOFF2Size(
        data: *const u8,
        length: usize,
        extended_metadata: *const core::ffi::c_char,
        extended_metadata_length: usize,
    ) -> usize;
    fn ConvertTTFToWOFF2(
        data: *const u8,
        length: usize,
        result: *mut u8,
        result_length: *mut usize,
        extended_metadata: *const core::ffi::c_char,
        extended_metadata_length: usize,
        brotli_quality: core::ffi::c_int,
        allow_transforms: core::ffi::c_int,
    ) -> core::ffi::c_int;
}

/// Compress.
pub fn compress(data: &[u8], metadata: String, quality: usize, transform: bool) -> Option<Vec<u8>> {
    let metadata_length = metadata.len();
    let metadata = match std::ffi::CString::new(metadata) {
        Result::Ok(metadata) => metadata,
        _ => return None,
    };
    let size = unsafe {
        ComputeTTFToWOFF2Size(
            data.as_ptr() as *const _,
            data.len(),
            metadata.as_ptr() as *const _,
            metadata_length,
        )
    };
    let mut result = vec![0; size];
    let mut result_length = result.len();
    let success = unsafe {
        ConvertTTFToWOFF2(
            data.as_ptr() as *const _,
            data.len(),
            result.as_mut_ptr() as *mut _,
            &mut result_length as *mut _,
            metadata.as_ptr() as *const _,
            metadata_length,
            quality as core::ffi::c_int,
            transform as core::ffi::c_int,
        ) != 0
    };
    if !success {
        return None;
    }
    result.truncate(result_length);
    result.into()
}
