//! 只读地问一遍 MusicBrainz + Deezer，看某个歌手能不能查到资料和照片。
//!
//! 用法：`cargo run -p jp-scraper --example probe_artist -- 月村手毬 [更多名字...]`
use jp_scraper::MetadataProvider;

fn main() {
    let names: Vec<String> = std::env::args().skip(1).collect();
    let (mb, deezer) = jp_scraper::artist_providers();
    for name in &names {
        println!("== {name}");
        let facts = match mb.get_artist(name, &[]) {
            Ok(v) => v,
            Err(err) => {
                println!("   MusicBrainz 出错: {err}");
                None
            }
        };
        let aliases = match &facts {
            Some(f) => {
                println!(
                    "   MusicBrainz: {} [{}] {} {} 别名 {:?}",
                    f.name, f.artist_type, f.country, f.formed, f.aliases
                );
                let mut names = f.aliases.clone();
                if !f.name.is_empty() {
                    names.push(f.name.clone());
                }
                names
            }
            None => {
                println!("   MusicBrainz: 查不到");
                Vec::new()
            }
        };
        match deezer.get_artist(name, &aliases) {
            Ok(Some(photo)) => println!("   Deezer: {} -> {}", photo.name, photo.image_url),
            Ok(None) => println!("   Deezer: 查不到"),
            Err(err) => println!("   Deezer 出错: {err}"),
        }
    }
}
