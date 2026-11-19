//! Development aid: write a file's preview images next to a path prefix.
//!   cargo run -p fuu-preview --example preview_dump -- <file> <out-prefix>
fn main() {
    let mut args = std::env::args().skip(1);
    let (file, prefix) = (args.next().expect("file"), args.next().expect("out prefix"));
    let data = std::fs::read(&file).expect("read");
    let message = fuu_preview::preview(&file, "", &data).expect("preview");
    if let Some(fuu_model::BodyPart::Html(html)) = message.body.first() {
        std::fs::write(format!("{prefix}.html"), html).unwrap();
    }
    for (i, image) in message.inline.iter().enumerate() {
        let ext = if image.mime == "image/png" { "png" } else { "jpg" };
        std::fs::write(format!("{prefix}-{i}.{ext}"), &image.data).unwrap();
    }
    println!("{} images", message.inline.len());
}
