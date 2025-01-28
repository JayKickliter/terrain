use criterion::{criterion_group, criterion_main, Criterion};
use itertools::Itertools;
use nasadem::Tile;
use std::{env, hint::black_box, path::PathBuf};

#[cfg(not(target_env = "msvc"))]
use tikv_jemallocator::Jemalloc;

#[cfg(not(target_env = "msvc"))]
#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;

fn three_arcsecond_tile_path() -> PathBuf {
    [
        env!("CARGO_MANIFEST_DIR"),
        "..",
        "data",
        "nasadem",
        "3arcsecond",
        "N44W072.hgt",
    ]
    .iter()
    .collect()
}

fn one_arcsecond_tile_path() -> PathBuf {
    [
        env!("CARGO_MANIFEST_DIR"),
        "..",
        "data",
        "nasadem",
        "1arcsecond",
        "N44W072.hgt",
    ]
    .iter()
    .collect()
}

fn nw_to_se(dim: usize) -> Vec<(usize, usize)> {
    let path = (0..dim).interleave(0..dim).tuples().collect::<Vec<_>>();
    assert_eq!(path.first(), Some(&(0, 0)));
    assert_eq!(path.last(), Some(&(dim - 1, dim - 1)));
    path
}

fn se_to_nw(dim: usize) -> Vec<(usize, usize)> {
    let mut path = nw_to_se(dim);
    path.reverse();
    path
}

fn ne_to_sw(dim: usize) -> Vec<(usize, usize)> {
    let path = (0..dim)
        .rev()
        .interleave(0..dim)
        .tuples()
        .collect::<Vec<_>>();
    assert_eq!(path.first(), Some(&(dim - 1, 0)));
    assert_eq!(path.last(), Some(&(0, dim - 1)));
    path
}

fn sw_to_ne(dim: usize) -> Vec<(usize, usize)> {
    let mut path = nw_to_se(dim);
    path.reverse();
    path
}

fn nw_to_sw(dim: usize) -> Vec<(usize, usize)> {
    let path = std::iter::repeat(0)
        .take(dim)
        .interleave(0..dim)
        .tuples()
        .collect::<Vec<_>>();
    assert_eq!(path.first(), Some(&(0, 0)));
    assert_eq!(path.last(), Some(&(0, dim - 1)));
    path
}

fn sw_to_nw(dim: usize) -> Vec<(usize, usize)> {
    let mut path = nw_to_se(dim);
    path.reverse();
    path
}

fn ne_to_se(dim: usize) -> Vec<(usize, usize)> {
    let path = std::iter::repeat(dim - 1)
        .take(dim)
        .interleave(0..dim)
        .tuples()
        .collect::<Vec<_>>();
    assert_eq!(path.first(), Some(&(dim - 1, 0)));
    assert_eq!(path.last(), Some(&(dim - 1, dim - 1)));
    path
}

fn se_to_ne(dim: usize) -> Vec<(usize, usize)> {
    let mut path = nw_to_se(dim);
    path.reverse();
    path
}

fn nw_to_ne(dim: usize) -> Vec<(usize, usize)> {
    let path = (0..dim)
        .interleave(std::iter::repeat(0).take(dim))
        .tuples()
        .collect::<Vec<_>>();
    assert_eq!(path.first(), Some(&(0, 0)));
    assert_eq!(path.last(), Some(&(dim - 1, 0)));
    path
}

fn ne_to_nw(dim: usize) -> Vec<(usize, usize)> {
    let mut path = nw_to_ne(dim);
    path.reverse();
    path
}

fn walk_tile(c: &mut Criterion, dim: usize, title: &str, tile: &Tile) {
    let mut group = c.benchmark_group(title);

    let cases = [
        ("NW→SE", nw_to_se(dim)),
        ("SE→NW", se_to_nw(dim)),
        ("NE→SW", ne_to_sw(dim)),
        ("SW→NE", sw_to_ne(dim)),
        ("NW→SW", nw_to_sw(dim)),
        ("SW→NW", sw_to_nw(dim)),
        ("NE→SE", ne_to_se(dim)),
        ("SE→NE", se_to_ne(dim)),
        ("NW→NE", nw_to_ne(dim)),
        ("NE→NW", ne_to_nw(dim)),
    ];

    for (bench_title, path) in cases {
        group.bench_with_input(bench_title, &(&tile, path), |b, (t, p)| {
            b.iter(|| {
                for &(x, y) in p {
                    let elev = t.get_unchecked((x, y));
                    black_box(elev);
                }
            });
        });
    }
}

fn walk_one_arcsecond_inmem_tile(c: &mut Criterion) {
    let tile = Tile::load(one_arcsecond_tile_path()).unwrap();
    walk_tile(c, 2048, "Walk in-memory 1-arcsecond tile", &tile);
}

fn walk_one_arcsecond_mmap_tile(c: &mut Criterion) {
    let tile = Tile::memmap(one_arcsecond_tile_path()).unwrap();
    walk_tile(c, 3601, "Walk in-memory 1-arcsecond tile", &tile);
}

fn walk_three_arcsecond_inmem_tile(c: &mut Criterion) {
    let tile = Tile::load(three_arcsecond_tile_path()).unwrap();
    walk_tile(c, 1201, "Walk in-memory 3-arcsecond tile", &tile);
}

fn walk_three_arcsecond_mmap_tile(c: &mut Criterion) {
    let tile = Tile::memmap(three_arcsecond_tile_path()).unwrap();
    walk_tile(c, 1201, "Walk in-memory 3-arcsecond tile", &tile);
}

criterion_group!(
    benches,
    walk_one_arcsecond_inmem_tile,
    walk_one_arcsecond_mmap_tile,
    walk_three_arcsecond_inmem_tile,
    walk_three_arcsecond_mmap_tile,
);
criterion_main!(benches);
