//! `model_tiers`: the verified manifest's tiers filtered by this Mac's RAM (from `system_profile`).
//! `available` = RAM >= the tier's minimum; `recommended` = the highest available tier. Download
//! text comes from manifest sizes only; when any size is unknown it says so (never a made-up number).

use super::model_manifest::{self, Manifest};
use serde::Serialize;

pub(crate) const SIZE_UNKNOWN: &str = "Size not published yet";

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ModelTier {
    pub id: String,
    pub name: String,
    pub min_ram_gb: u32,
    pub models: Vec<String>,
    pub download: String,
    pub note: Option<String>,
    pub recommended: bool,
    pub available: bool,
}

fn download_text(models: &[model_manifest::ManifestModel]) -> String {
    let mut total: u64 = 0;
    for m in models {
        match m.bytes {
            Some(b) => total = total.saturating_add(b),
            None => return SIZE_UNKNOWN.to_string(),
        }
    }
    format!("{:.1} GB", total as f64 / 1e9)
}

pub(crate) fn tiers_for(m: &Manifest, ram_gb: u32) -> Vec<ModelTier> {
    let mut tiers: Vec<_> = m.tiers.iter().collect();
    tiers.sort_by_key(|t| t.min_ram_gb);
    let best = tiers
        .iter()
        .rfind(|t| ram_gb >= t.min_ram_gb)
        .map(|t| t.id.clone());
    tiers
        .into_iter()
        .map(|t| ModelTier {
            id: t.id.clone(),
            name: t.name.clone(),
            min_ram_gb: t.min_ram_gb,
            models: t.models.iter().map(|x| x.name.clone()).collect(),
            download: download_text(&t.models),
            note: None,
            recommended: best.as_deref() == Some(t.id.as_str()),
            available: ram_gb >= t.min_ram_gb,
        })
        .collect()
}

#[tauri::command]
#[specta::specta]
pub async fn model_tiers() -> Result<Vec<ModelTier>, String> {
    let m = model_manifest::load().map_err(str::to_string)?;
    let p = super::system_profile::system_profile().await?;
    Ok(tiers_for(&m, p.ram_gb))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shipped() -> Manifest {
        model_manifest::load().unwrap()
    }

    fn summary(ram: u32) -> (Vec<String>, Option<String>) {
        let t = tiers_for(&shipped(), ram);
        (
            t.iter()
                .filter(|x| x.available)
                .map(|x| x.id.clone())
                .collect(),
            t.iter().find(|x| x.recommended).map(|x| x.id.clone()),
        )
    }

    #[test]
    fn tiers_by_ram() {
        assert_eq!(summary(8), (vec!["compact".into()], Some("compact".into())));
        assert_eq!(
            summary(16),
            (vec!["compact".into()], Some("compact".into()))
        );
        assert_eq!(
            summary(32),
            (
                vec!["compact".into(), "standard".into()],
                Some("standard".into())
            )
        );
        assert_eq!(summary(36).1, Some("standard".into()));
        assert_eq!(summary(64).1, Some("large".into()));
        assert_eq!(summary(64).0.len(), 3);
        assert_eq!(summary(4), (vec![], None));
    }

    #[test]
    fn live_names_only_and_download_unknown_not_invented() {
        let t = tiers_for(&shipped(), 36);
        let all: Vec<&String> = t.iter().flat_map(|x| &x.models).collect();
        assert!(all.iter().any(|n| *n == "gemma4:latest"));
        assert!(all.iter().any(|n| *n == "qwen3.8-uncensored:latest"));
        assert!(all.iter().any(|n| *n == "nomic-embed-text:latest"));
        assert!(!all.iter().any(|n| n.contains("gemma4-12b")));
        assert!(t.iter().all(|x| x.download == SIZE_UNKNOWN));
    }

    #[test]
    fn download_text_sums_known_sizes() {
        let mut m = shipped();
        for md in &mut m.tiers[0].models {
            md.bytes = Some(1_500_000_000);
        }
        let t = tiers_for(&m, 8);
        assert_eq!(t[0].download, "3.0 GB");
    }

    #[test]
    fn shape_is_camel_case() {
        let j = serde_json::to_value(&tiers_for(&shipped(), 36)[0]).unwrap();
        for k in [
            "id",
            "name",
            "minRamGb",
            "models",
            "download",
            "recommended",
            "available",
        ] {
            assert!(j.get(k).is_some(), "{k}");
        }
    }
}
