//! Regenerate the app icons from `icons/app/fuu.svg`:
//!
//!     cargo run -p fuu-app --example make_icons
//!
//! Writes, next to the SVG:
//! - `fuu.icns`: the full artwork (tile with margin and shadow, as macOS
//!   expects on its icon grid), 16–1024 px.
//! - `fuu.ico` and `fuu-<size>.png`: the tile cropped close (Windows icons
//!   fill their square), for the exe, the installer and the window icon.

use std::path::Path;

use resvg::{tiny_skia, usvg};

/// Crop around the tile (x 132–881, y 141–889 in the 1024 artwork), keeping
/// a little room for the drop shadow.
const TILE_VIEW: (f32, f32, f32) = (116.0, 122.0, 782.0);

fn render(tree: &usvg::Tree, size: u32, view: Option<(f32, f32, f32)>) -> Vec<u8> {
    let mut pixmap = tiny_skia::Pixmap::new(size, size).expect("pixmap");
    let (x, y, side) = view.unwrap_or((0.0, 0.0, tree.size().width()));
    let scale = size as f32 / side;
    let transform = tiny_skia::Transform::from_row(scale, 0.0, 0.0, scale, -x * scale, -y * scale);
    resvg::render(tree, transform, &mut pixmap.as_mut());
    pixmap.encode_png().expect("png")
}

/// An ICO holding PNG images (Vista and later).
fn ico(images: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&[0, 0, 1, 0]);
    out.extend_from_slice(&(images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len() as u32;
    for (size, png) in images {
        let side = if *size >= 256 { 0 } else { *size as u8 };
        out.extend_from_slice(&[side, side, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes()); // colour planes
        out.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
        out.extend_from_slice(&(png.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += png.len() as u32;
    }
    for (_, png) in images {
        out.extend_from_slice(png);
    }
    out
}

/// An ICNS holding PNG images.
fn icns(images: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let body_len: usize = images.iter().map(|(_, png)| 8 + png.len()).sum();
    let mut out = Vec::with_capacity(8 + body_len);
    out.extend_from_slice(b"icns");
    out.extend_from_slice(&((8 + body_len) as u32).to_be_bytes());
    for (kind, png) in images {
        out.extend_from_slice(*kind);
        out.extend_from_slice(&((8 + png.len()) as u32).to_be_bytes());
        out.extend_from_slice(png);
    }
    out
}

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("icons/app");
    let svg = std::fs::read(dir.join("fuu.svg")).expect("read fuu.svg");
    let tree = usvg::Tree::from_data(&svg, &usvg::Options::default()).expect("parse fuu.svg");

    let tile: Vec<(u32, Vec<u8>)> = [16, 20, 24, 32, 40, 48, 64, 128, 256]
        .into_iter()
        .map(|size| (size, render(&tree, size, Some(TILE_VIEW))))
        .collect();
    std::fs::write(dir.join("fuu.ico"), ico(&tile)).expect("write ico");
    for (size, png) in tile.iter().filter(|(s, _)| [32, 128, 256].contains(s)) {
        std::fs::write(dir.join(format!("fuu-{size}.png")), png).expect("write png");
    }

    let full = |size| render(&tree, size, None);
    let mac: Vec<(&[u8; 4], Vec<u8>)> = vec![
        (b"icp4", full(16)),
        (b"ic11", full(32)), // 16@2x
        (b"icp5", full(32)),
        (b"ic12", full(64)), // 32@2x
        (b"ic07", full(128)),
        (b"ic13", full(256)), // 128@2x
        (b"ic08", full(256)),
        (b"ic14", full(512)), // 256@2x
        (b"ic09", full(512)),
        (b"ic10", full(1024)), // 512@2x
    ];
    std::fs::write(dir.join("fuu.icns"), icns(&mac)).expect("write icns");
    println!("icons written to {}", dir.display());
}
