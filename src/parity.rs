//! Rastreador de paridade Electron x TUI (`parity.json` embutido).
//!
//! Cada feature tem status done|partial|missing|out-of-scope + evidência.
//! O teste `matriz_valida` impõe as regras para ninguém marcar done no grito.

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Feature {
    pub id: String,
    pub area: String,
    pub status: String,
    pub evidence: String,
    pub electron: String,
    pub tui: String,
}

#[derive(Debug, Clone, Deserialize)]
struct Matrix {
    pub version: u32,
    #[allow(dead_code)]
    pub note: String,
    pub features: Vec<Feature>,
}

pub fn load() -> Vec<Feature> {
    let raw = include_str!("../parity.json");
    let m: Matrix = serde_json::from_str(raw).expect("parity.json válido");
    assert_eq!(m.version, 1, "versão da matriz");
    m.features
}

pub struct Summary {
    pub done: usize,
    pub partial: usize,
    pub missing: usize,
    pub out_of_scope: usize,
    pub total: usize,
}

impl Summary {
    /// % ponderado (done=1, partial=0.5) sobre o escopo (sem out-of-scope).
    pub fn percent(&self) -> u16 {
        let scope = (self.total - self.out_of_scope).max(1) as f64;
        (((self.done as f64 + self.partial as f64 * 0.5) / scope) * 100.0).round() as u16
    }
}

pub fn summarize(fs: &[Feature]) -> Summary {
    let mut s = Summary {
        done: 0,
        partial: 0,
        missing: 0,
        out_of_scope: 0,
        total: fs.len(),
    };
    for f in fs {
        match f.status.as_str() {
            "done" => s.done += 1,
            "partial" => s.partial += 1,
            "missing" => s.missing += 1,
            "out-of-scope" => s.out_of_scope += 1,
            _ => {}
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn matriz_valida() {
        let fs = load();
        assert!(!fs.is_empty());
        let mut ids = HashSet::new();
        for f in &fs {
            assert!(ids.insert(f.id.clone()), "id duplicado: {}", f.id);
            assert!(
                ["done", "partial", "missing", "out-of-scope"].contains(&f.status.as_str()),
                "status inválido em {}",
                f.id
            );
            assert!(
                ["unit", "fixture", "live", "manual", "none"].contains(&f.evidence.as_str()),
                "evidence inválida em {}",
                f.id
            );
            if f.status == "done" {
                assert_ne!(f.evidence, "none", "done sem evidência: {}", f.id);
            }
            assert!(
                !f.area.is_empty() && !f.electron.is_empty(),
                "campos vazios em {}",
                f.id
            );
        }
        let s = summarize(&fs);
        assert!(s.done + s.partial + s.missing + s.out_of_scope == s.total);
        assert!(s.percent() <= 100);
    }
}
