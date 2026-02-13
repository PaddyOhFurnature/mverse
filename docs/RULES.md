# DEVELOPMENT RULES — NON-NEGOTIABLE

**Purpose:** Hard constraints that must never be violated. Read before writing any code.
**Last Updated:** 2026-02-13

---

## RULE 1: TESTS BEFORE CODE

Never write implementation code without a failing test first.

```
1. Write the test that describes expected behaviour
2. Run it — it MUST fail (red)
3. Write the minimum code to make it pass (green)
4. Refactor if needed (clean)
5. Run ALL tests — they must ALL pass
6. Commit
```

If you cannot write a test for it, you do not understand it well enough to implement it.

---

## RULE 2: ALL TESTS MUST PASS — ALWAYS

```bash
cargo test --lib -- --nocapture
```

If ANY test fails, STOP. Fix it before doing anything else.

- Do not comment out failing tests
- Do not skip tests with `#[ignore]` without documenting why in the test and in a code comment
- Do not commit with failing tests
- Do not say "I will fix it later"
- Do not proceed to the next task

---

## RULE 3: SCALE GATE TESTING

Every change must be verified at multiple scales. Nothing progresses until it works at the current scale gate level.

Testing radii (in order):
1. Single entity (1 building, 1 road segment)
2. City block (~200m radius)
3. 1 km radius
4. 10 km radius
5. 100 km radius
6. State / province
7. Country
8. Globe (full sphere)

The current phase determines which gates must pass. See `docs/TESTING.md` for the full gate-to-phase mapping.

---

## RULE 4: NEVER MODIFY COORDINATE MATH WITHOUT EXTENSIVE TESTING

The coordinate system is the foundation of everything. If coordinates are wrong, the entire world is wrong.

Before modifying ANY code in `coordinates.rs`:
1. Document WHY the change is needed
2. Write tests showing the new behaviour is correct
3. Verify round-trip accuracy: GPS → ECEF → GPS < 1mm error
4. Verify landmark distances match reference values
5. Verify ENU conversions are accurate
6. Run performance benchmarks (must maintain throughput targets)

After modifying coordinate code:
1. Run ALL coordinate tests
2. Run ALL chunk tests (chunks depend on coordinates)
3. Run ALL world/renderer tests (everything depends on coordinates)
4. Run visual verification (if renderer exists)
5. Run scale gate tests at current level

---

## RULE 5: NEVER OPTIMISE WITHOUT MEASURING

```
Is it measured as slow?
  NO  → Do not touch it
  YES → Profile to find the ACTUAL bottleneck (not what you guess)
        → Optimise ONLY that specific part
        → Benchmark before AND after
        → If no measurable improvement → revert
        → If regression in correctness → revert
```

Tools for measurement:
- `cargo bench` for microbenchmarks
- `cargo flamegraph` for profiling
- `cargo test --release` for release-mode benchmarks
- `top` / `htop` for memory usage
- Window title FPS counter for rendering

---

## RULE 6: CORRECTNESS → PERFORMANCE → READABILITY → SIMPLICITY

When these conflict, choose in this order:

1. **Correctness** — passes all tests, no bugs, deterministic, handles edge cases
2. **Performance** — meets FPS and throughput targets for current phase
3. **Readability** — another developer or AI can understand it in 60 seconds
4. **Simplicity** — fewer lines, less complexity, less abstraction
5. **Cleverness** — NEVER sacrifice 1-4 for clever tricks, one-liners, or "elegant" solutions

---

## RULE 7: DOCUMENT EVERY PUBLIC FUNCTION

Every `pub fn`, `pub struct`, and `pub enum` must have a doc comment:

```rust
/// Converts a GPS position to ECEF (Earth-Centered Earth-Fixed) coordinates.
///
/// Uses the WGS84 ellipsoid model. The resulting ECEF position is in metres,
/// with origin at Earth's centre.
///
/// # Arguments
/// * `gps` - GPS position with latitude/longitude in degrees and elevation in metres
///
/// # Returns
/// ECEF position in metres
///
/// # Examples
/// ```
/// let gps = GpsPos { lat_deg: -27.4698, lon_deg: 153.0251, elevation_m: 0.0 };
/// let ecef = gps_to_ecef(&gps);
/// // ecef.x ≈ -5046125, ecef.y ≈ 2568335, ecef.z ≈ -2924861
/// ```
pub fn gps_to_ecef(gps: &GpsPos) -> EcefPos {
    // ...
}
```

No exceptions. If a function is not worth documenting, it is not worth making public.

---

## RULE 8: RESPECT API RATE LIMITS

All external API calls MUST:
- Have a minimum cooldown between requests (3 seconds for Overpass API)
- Use exponential backoff on failure (3s → 6s → 12s → 24s → max 60s)
- Check disk cache BEFORE making any network request
- Set a custom User-Agent header identifying this project
- Handle timeouts gracefully (do not crash, do not hang, do not retry infinitely)
- Log every API call: URL, response status, duration
- Limit concurrent requests (max 2 for Overpass)

Violating rate limits will get the project's IP banned from public APIs. This is not recoverable.

---

## RULE 9: THE WORLD IS A SPHERE

Never use planar/flat approximations for anything that spans more than one chunk.

- Within a single chunk (<500m): ENU (local Cartesian) is acceptable
- Across chunks: MUST use ECEF or geodetic calculations on the sphere
- At city scale (>10km): Earth curvature is visually noticeable and mathematically significant
- At country scale: flat approximations produce errors of hundreds of kilometres

If you find yourself writing `x = longitude * some_constant` for cross-chunk work, you are doing it wrong. Stop. Use ECEF.

---

## RULE 10: DETERMINISM IS MANDATORY

All simulation and world-state operations MUST be deterministic:
- Same inputs → same outputs on ALL instances (same platform)
- Fixed-point or carefully controlled floating-point arithmetic for shared state
- No reliance on `HashMap` iteration order for logic (use `BTreeMap` or sort first)
- No reliance on system wall-clock time for simulation (use logical clocks: Lamport or HLC)
- Physics uses fixed timestep (60 Hz, 16.67ms per tick)
- SVO operations produce identical results for identical op sequences
- Content hashes (SHA-256) verify state consistency between peers

Why: two peers simulating the same chunk must arrive at the same state. If they diverge, the world breaks and players see different realities.

---

## RULE 11: COMMIT MESSAGES MUST BE DESCRIPTIVE

Format: `type(scope): description`

Types:
- `feat` — new feature or functionality
- `fix` — bug fix
- `test` — adding or modifying tests
- `perf` — performance improvement (with measurements)
- `refactor` — code restructuring (no behaviour change)
- `docs` — documentation changes
- `chore` — build, CI, dependency updates

Examples:
```
feat(coords): implement GPS to ECEF conversion with WGS84 ellipsoid
test(chunks): add quad-sphere depth-14 tile boundary verification
fix(osm): handle missing building height tag, default to 9m
perf(svo): reduce allocation count in octree insertion by 40%
refactor(world): extract chunk loading into separate async function
docs(readme): update current status to Phase 3 complete
chore(deps): update wgpu to 24.0.1
```

Bad examples (DO NOT):
```
fix stuff
update
wip
changes
asdf
```

---

## RULE 12: NO UNSAFE RUST WITHOUT JUSTIFICATION

`unsafe` blocks are permitted ONLY when ALL of these are true:
1. A safe alternative does not exist for the required functionality
2. The performance difference is measured and significant (>20% improvement on a hot path)
3. The unsafe code is isolated in a clearly marked function
4. The function has a `# Safety` doc comment explaining ALL invariants
5. The function has dedicated tests proving it does not cause undefined behaviour
6. A comment at the `unsafe` block explains exactly why it is safe in this specific context

If you can do it safely, do it safely. Rust's whole point is safe systems programming.

---

## RULE 13: DEPENDENCIES MUST BE JUSTIFIED

Before adding any new dependency to `Cargo.toml`:
1. Verify the crate is actively maintained (last commit < 6 months ago)
2. Verify it has reasonable download count (>10,000 downloads or well-known ecosystem crate)
3. Verify it does not pull in an excessive transitive dependency tree
4. Document WHY this crate is needed (comment in Cargo.toml)
5. Prefer the Rust ecosystem standard: `serde`, `rayon`, `tokio`, `wgpu`, `rapier`, `libp2p` are all acceptable

Do not add a crate to do something you can write in 20 lines of straightforward code.

---

## RULE 14: ERROR HANDLING — NO PANICS IN PRODUCTION CODE

- Use `Result<T, E>` for operations that can fail
- Use `Option<T>` for values that may not exist
- `unwrap()` is permitted ONLY in tests and examples
- Production code must use `?`, `.map_err()`, or explicit match
- Never `panic!()` on invalid input — return an error
- Log errors with context (what operation failed, what input caused it, what to do about it)

The only acceptable panic is a programmer error (logic bug) caught by `debug_assert!()` in debug builds.

---

## RULE 15: GIT HYGIENE

- Commit frequently (at least after each passing test or completed sub-task)
- Each commit should compile and pass all tests
- Never commit generated files (`target/`, cache files)
- `.gitignore` must include: `target/`, `*.swp`, `.DS_Store`, `~/.metaverse/cache/`
- Branch for experimental work; merge only when tests pass
- Tag releases: `v0.1.0`, `v0.2.0`, etc. (semver)