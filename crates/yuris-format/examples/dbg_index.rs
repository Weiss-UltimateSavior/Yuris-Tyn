use yuris_format::ypf::YpfIndex;

fn main() {
    let idx = YpfIndex::from_path(
        std::path::Path::new(r"AnimalTrailGirlishSquare 2\pac\cg.ypf"),
        0xC9,
    )
    .unwrap();
    println!("count={} names={}", idx.header.file_count, idx.names.len());
    println!("flags[:10]={:?}", &idx.flags[..10]);
    for e in idx.entries.iter().take(2) {
        println!(
            "name={:?} flag={:#x} uncomp={} comp={} off={:#x}",
            String::from_utf8_lossy(&e.name),
            e.flag,
            e.uncompressed_len,
            e.compressed_len,
            e.offset
        );
    }
}
