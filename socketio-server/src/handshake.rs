//TODO: add http headerMap
/// Handshake informations bound to a socket
pub struct Handshake
{
    pub auth: Option<serde_json::Value>,
    pub url: String,
    // pub headers: HeaderMap<HeaderValue>,
    pub issued: u64,
}
