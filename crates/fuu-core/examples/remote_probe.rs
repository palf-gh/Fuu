//! Diagnostic: fetch image URLs with Fuu's remote-image client. `-- url...`
#[tokio::main(flavor = "current_thread")]
async fn main() {
    let urls: Vec<String> = std::env::args().skip(1).collect();
    for (url, result) in urls.iter().zip(fuu_core::remote::fetch_images(&urls).await) {
        match result {
            Some((mime, bytes)) => println!("OK {mime} {} bytes  {url}", bytes.len()),
            None => println!("REFUSED/FAILED  {url}"),
        }
    }
}
