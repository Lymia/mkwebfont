use crate::{data::DataStorage, WebfontInfo};
use anyhow::Result;
use mkwebfont_common::join_set::JoinSet;
use mkwebfont_extract_web::RewriteContext;
use std::sync::Arc;

pub struct FontReport {
    heading: String,
    css_bytes: usize,
    report: Vec<ScriptReport>,
}
impl FontReport {
    pub async fn for_font(style: &WebfontInfo) -> Result<FontReport> {
        make_reports(style).await
    }

    pub fn print(&self) {
        print_report(&self.heading, self.css_bytes, &self.report);
    }
}

struct ScriptReport {
    name: String,
    valid: f64,
    files: f64,
    avg_kib: f64,
    avg_kib_all: f64,
    missing: f64,
}

#[rustfmt::skip]
fn print_report(heading: &str, css_bytes: usize, data: &[ScriptReport]) {
    eprintln!();
    eprintln!("===================================================================================");
    eprintln!("Report for font: {heading}");
    eprintln!("===================================================================================");
    eprintln!("Script          | %Valid | Avg.Miss | Avg.Files | Avg.Size (Valid) | Avg.Size (All)");
    eprintln!("----------------+--------+----------+-----------+------------------+---------------");
    for ScriptReport { name, valid, files, avg_kib, avg_kib_all, missing } in data {
        let avg_kib = if avg_kib.is_nan() {
            "--".to_string()
        } else {
            format!("{avg_kib:.2}")
        };
        let avg_kib_all = if avg_kib_all.is_nan() {
            "--".to_string()
        } else {
            format!("{avg_kib_all:.2}")
        };
        eprint!("{name:-15} | {valid:5.1}% | {missing:8.2} | {files:9.3} | ");
        eprintln!("{avg_kib:>12} KiB | {avg_kib_all:>10} KiB");
    }
    let css_size = css_bytes as f64 / 1024.0;
    eprintln!("----------------+--------+----------+-----------+------------------+---------------");
    eprintln!("(CSS)           |        |          |           | {css_size:12.2} KiB |");
    eprintln!("===================================================================================");
}

async fn make_reports(style: &WebfontInfo) -> Result<FontReport> {
    let chars_in_font = style.all_chars();
    let style = Arc::new(style.clone());
    let mut joins = JoinSet::new();
    for section in DataStorage::instance()?.validation_list().await?.sections() {
        let chars_in_font = chars_in_font.clone();
        let section = section.clone();
        let style = style.clone();
        joins.spawn(async move {
            let mut total_size_valid = 0u64;
            let mut total_size_all = 0u64;
            let mut total_missing = 0u64;
            let mut valid_count = 0usize;
            let mut total_subsets = 0usize;
            for bitmap in section.iter() {
                let bitmap = section.decode(bitmap);

                let mut size = 0u64;
                for subset in style.subsets() {
                    if bitmap.intersection_len(subset.subset()) > 0 {
                        size += subset.woff2_data().len() as u64;
                        total_subsets += 1;
                    }
                }

                let missing = bitmap.len() - bitmap.intersection_len(&chars_in_font);
                if missing == 0 {
                    total_size_valid += size;
                    valid_count += 1;
                }
                total_size_all += size;
                total_missing += missing;
            }

            let name = section.source();
            let valid = (valid_count as f64 / section.len() as f64) * 100.0;
            let files = total_subsets as f64 / section.len() as f64;
            let avg_kib = (total_size_valid as f64 / valid_count as f64) / 1024.0;
            let avg_kib_all = (total_size_all as f64 / section.len() as f64) / 1024.0;
            let missing = total_missing as f64 / section.len() as f64;

            Ok(
                ScriptReport {
                    name: name.to_string(),
                    valid,
                    files,
                    avg_kib,
                    avg_kib_all,
                    missing,
                },
            )
        });
    }

    let heading = format!("{} {}", style.font_family(), style.font_style());
    let ctx = RewriteContext { webfonts: vec![style], ..RewriteContext::default() };
    let css_bytes = ctx.generate_font_css()?.len();
    let report = joins.join().await?;
    Ok(FontReport { heading, css_bytes, report })
}
