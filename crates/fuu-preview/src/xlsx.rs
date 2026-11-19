use crate::{Output, PreviewError};
pub(crate) fn preview(_data: &[u8]) -> Result<Output, PreviewError> { Err(PreviewError::Unsupported) }
