# BLOCKER: Cannot Verify Changes Visually

**Date:** 2026-02-15 14:00 UTC  
**Status:** BLOCKED - Must fix before continuing

## The Problem

User is right: "if you cant see what your doing, we stop now. there is zero point"

I cannot see what's rendering because:

1. **Environment is headless SSH** - No X11 display, can't run viewer interactively
2. **Screenshot tools are outdated** - All use old `svo_integration` direct mesh generation
3. **Current architecture uses WorldManager** - Screenshot tools don't know how to use it
4. **Can't verify anything** - Been trusting console output ("115K vertices!") instead of looking

## What Exists But Doesn't Work

### reference/ directory
- 10 reference photos from Google Earth
- Exact camera positions and angles documented in REFERENCE_IMAGES.md
- Story Bridge, Brisbane (-27.463697°, 153.035725°)
- User set this up for visual comparison

### screenshot/ directory  
- Should contain our rendered output matching reference angles
- Currently has old screenshots from previous (different) system
- Need to regenerate with current WorldManager architecture

### examples/capture_screenshots.rs
- **Status:** Uses old svo_integration (direct mesh)
- **Problem:** Doesn't use WorldManager/SVO pipeline
- **Can't use:** Architecture mismatch

### examples/screenshot_test.rs
- **Status:** Uses old svo_integration (direct mesh)
- **Problem:** Doesn't use WorldManager/SVO pipeline
- **Can't use:** Architecture mismatch

## What Needs To Happen

### Option 1: Rewrite Screenshot Tool (RECOMMENDED)

Create `examples/screenshot_worldmanager.rs`:
```rust
// Uses WorldManager (current architecture)
// Takes 10 screenshots matching REFERENCE_IMAGES.md positions
// Saves to screenshot/*.png for comparison
// Runs headless with xvfb
```

**Requirements:**
- Initialize WorldManager same way viewer does
- Set camera to each reference position
- Let WorldManager generate chunks
- Render one frame
- Save PNG to screenshot/
- Repeat for all 10 positions

**Estimate:** 2-3 hours to write and debug

### Option 2: Remote Display (ALTERNATIVE)

Set up X11 forwarding or VNC so user can run viewer interactively

**Problems:**
- Requires SSH with X11 forwarding (-X flag)
- Performance over network
- May not have permission/setup

### Option 3: User Tests Locally (FALLBACK)

User runs viewer on their machine with display and reports results

**Problems:**
- Slower iteration cycle
- User shouldn't have to do my testing
- Defeats purpose of reference photo system

## Current State

**What works:**
- WorldManager architecture (generates terrain in chunks)
- Marching cubes at LOD 0-1
- Chunk positioning (fixed from 4km to 555m)
- Test harnesses prove pipeline works

**What doesn't work:**
- Visual verification (screenshot system broken)
- LOD 2-4 (marching cubes bug)
- Frustum culling (not implemented)
- Multi-chunk loading (only 1 chunk)

**Can't proceed without:**
- Ability to SEE rendered output
- Compare to reference photos
- Verify changes improve (not worsen) visuals

## Decision Point

User says: "you need to do them in order, if you cant see what your doing, we stop now"

**I agree completely.** 

Next step: Rewrite screenshot capture to use WorldManager, or stop until user can test interactively.

Waiting for direction.
