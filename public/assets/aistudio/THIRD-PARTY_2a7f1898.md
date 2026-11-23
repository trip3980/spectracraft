# Dependencies and reference algorithms

Cargo.lock fixes all crate versions and registry checksums. The core dependency tree and optional bench tree are recorded in evidence/rust-dependencies-core.txt and evidence/rust-dependencies-bench.txt. Optional desktop dependencies use the operating system's windowing, graphics, dialogs and audio interfaces. No interpreter or external decoder/meter is spawned by the DSP/bench paths. The preflight deliberately invokes Cargo for build/audit checks.

The reference-compatible uniform generator in src/dsp.rs is an independent Rust implementation of PCG XSL RR 128/64 with SeedSequence expansion. Its seven raw test values are from NumPy's public pcg64-testset-1.csv (seed 0xdeadbeaf). Reference sources:

- https://github.com/numpy/numpy/blob/main/numpy/random/src/pcg64/pcg64.h
- https://github.com/numpy/numpy/blob/main/numpy/random/bit_generator.pyx
- https://github.com/numpy/numpy/blob/main/numpy/random/tests/data/pcg64-testset-1.csv
- NumPy licensing: https://github.com/numpy/numpy/blob/main/LICENSE.txt
- PCG algorithm: https://www.pcg-random.org/

The pinned ebur128 0.1.10 crate implements integrated loudness and true peak in Rust. See https://docs.rs/ebur128/0.1.10/ebur128/struct.EbuR128.html and its crate license. The meter's true-peak interpolation is implementation-defined; bench tests validate constant-gain behavior and matched export tolerances, not equivalence to every FFmpeg version.

Crate sources and their licenses remain in the Cargo registry and are not vendored. The source bundle does not redistribute the user audio or third-party sample libraries. Reproducing asset-dependent benches requires the original supplied fixtures and preset archives.
