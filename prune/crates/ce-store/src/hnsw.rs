use anyhow::{anyhow, Result};
use hnsw_rs::api::AnnT;
use hnsw_rs::hnswio::{load_description, HnswIo, ReloadOptions};
use hnsw_rs::prelude::*;
use ouroboros::self_referencing;
use std::fs::File;
use std::path::{Path, PathBuf};

/// In-process HNSW index wrapper.
///
/// This supports two backing modes:
/// - **Owned**: built from SQLite embeddings and fully owned in memory.
/// - **Loaded**: loaded from a prior `file_dump` on disk.
///
/// NOTE: `hnsw_rs`'s reload API returns an HNSW whose lifetime is tied to the loader.
/// We use a small self-referential struct (via `ouroboros`) to keep the loader alive.
pub struct VecIndex {
    pub dim: usize,
    inner: VecIndexInner,
}

enum VecIndexInner {
    Owned(Hnsw<'static, f32, DistCosine>),
    Loaded(LoadedIndex),
}

#[self_referencing]
struct LoadedIndex {
    io: HnswIo,
    #[borrows(io)]
    #[not_covariant]
    hnsw: Hnsw<'this, f32, DistCosine>,
}

impl VecIndex {
    pub fn new(dim: usize, expected: usize) -> Self {
        let max_nb_conn = 32;
        let max_layer = 16;
        let ef_construction = 200;
        let dist = DistCosine {};
        let hnsw = Hnsw::new(max_nb_conn, expected, max_layer, ef_construction, dist);
        Self { dim, inner: VecIndexInner::Owned(hnsw) }
    }

    /// Return the on-disk dump paths that `hnsw_rs` uses.
    pub fn dump_paths(dir: &Path, base: &str) -> (PathBuf, PathBuf) {
        let graph = dir.join(format!("{base}.hnsw.graph"));
        let data = dir.join(format!("{base}.hnsw.data"));
        (graph, data)
    }

    pub fn dump_exists(dir: &Path, base: &str) -> bool {
        let (graph, data) = Self::dump_paths(dir, base);
        graph.exists() && data.exists()
    }

    /// Read only the dump description (cheap). Useful for staleness checks.
    pub fn dump_description(dir: &Path, base: &str) -> Result<hnsw_rs::hnswio::Description> {
        let (graph, _data) = Self::dump_paths(dir, base);
        let mut f = File::open(&graph)
            .map_err(|e| anyhow!("failed to open HNSW graph dump {:?}: {e}", graph))?;
        let desc = load_description(&mut f)?;
        Ok(desc)
    }

    /// Try to load a previously dumped HNSW index from disk.
    ///
    /// This is typically much faster than rebuilding from SQLite embeddings.
    pub fn try_load(dir: &Path, base: &str, mmap: bool) -> Result<Self> {
        if !Self::dump_exists(dir, base) {
            return Err(anyhow!("HNSW dump not found at dir={:?} base={base}", dir));
        }

        // Derive dim from the dump description.
        let desc = Self::dump_description(dir, base)?;
        let dim = desc.dimension;

        let opts = ReloadOptions::new(mmap);
        let io = HnswIo::new_with_options(dir, base, opts);

        // Use `load_hnsw_with_dist` to avoid relying on `DistCosine: Default`.
        let inner = LoadedIndexTryBuilder {
            io,
            hnsw_builder: |io: &HnswIo| io.load_hnsw_with_dist::<f32, DistCosine>(DistCosine {}),
        }
        .try_build()?;

        Ok(Self { dim, inner: VecIndexInner::Loaded(inner) })
    }

    pub fn insert(&mut self, label: usize, v: &[f32]) {
        match &mut self.inner {
            VecIndexInner::Owned(hnsw) => {
                hnsw.insert_slice((v, label));
            }
            VecIndexInner::Loaded(_) => {
                // We don't currently support incremental updates to a loaded index.
                // The caller should rebuild from embeddings and dump a fresh copy.
                //
                // If this happens, it's a programming error.
                debug_assert!(false, "attempted to insert into a loaded VecIndex");
            }
        }
    }

    pub fn search(&self, q: &[f32], k: usize, ef_search: usize) -> Vec<Neighbour> {
        match &self.inner {
            VecIndexInner::Owned(hnsw) => hnsw.search(q, k, ef_search),
            VecIndexInner::Loaded(li) => li.with_hnsw(|h| h.search(q, k, ef_search)),
        }
    }

    pub fn dump(&self, dir: &Path, base: &str) -> Result<String> {
        match &self.inner {
            VecIndexInner::Owned(hnsw) => Ok(hnsw.file_dump(dir, base)?),
            VecIndexInner::Loaded(li) => li.with_hnsw(|h| h.file_dump(dir, base)),
        }
    }
}
