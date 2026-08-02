use zip::result::ZipError;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("zip io error: {0}")]
    ZipIo(#[from] zip::result::ZipError),

    #[error("missing relation of root part in _rels/.rels")]
    MissingRootPart,

    #[error("missing file {1} in 3mf: {0}")]
    MissingFile(#[source] ZipError, String),

    #[error("xml encoding error {0}")]
    Encoding(#[from] quick_xml::encoding::EncodingError),

    #[error("xml format error {0}")]
    XmlFormat(#[from] quick_xml::Error),

    #[error("invalid utf-8 in target path: {0}")]
    InvalidUtf8(#[from] std::string::FromUtf8Error),

    #[error("invalid utf-8 in attribute: {0}")]
    WrongUtf8(#[from] std::str::Utf8Error),

    #[error("Wrong float format: {0}")]
    FloatFormat(#[from] fast_float2::Error),

    //#[error("Wrong integer format: {0}")]
    //IntFormatError(#[from] ParseIntError),
    #[error("Invalid content type for file {0}")]
    InvalidContentType(String),

    #[error("Missing attribute {0} in element {1}")]
    MissingAttribute(String, String),

    #[error("Geometry found outside object")]
    NoOpenObject,

    #[error("Out of memory: could not reserve memory for data")]
    OutOfMemory(#[from] std::collections::TryReserveError),
}
