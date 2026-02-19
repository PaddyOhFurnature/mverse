#!/bin/bash
set -e

echo "=== Project Cleanup ==="
echo ""

# Move session notes
echo "Moving session notes..."
mv SESSION_SUMMARY_*.md archive/session-notes/ 2>/dev/null || true
mv AGENTS.md DIAGNOSIS.md archive/session-notes/ 2>/dev/null || true
mv NEXT_STEPS*.md PHASE*.md PROFILING_RESULTS.md archive/session-notes/ 2>/dev/null || true
mv RENDERING_*.md SCREENSHOT_*.md archive/session-notes/ 2>/dev/null || true
mv SRTM_DOWNLOADER_STATUS.md SRTM_LOADING_ISSUE.md SRTM_SOURCES_2026.md archive/session-notes/ 2>/dev/null || true
mv SVO_PIPELINE_STATUS.md archive/session-notes/ 2>/dev/null || true
mv TERRAIN_ISSUE_ANALYSIS.md TERRAIN_RENDERING_*.md archive/session-notes/ 2>/dev/null || true
mv VALIDATION_COMPLETE.md VIEWER_*.md VISUAL_*.md archive/session-notes/ 2>/dev/null || true
mv CAMERA_ORIENTATION_FIX.md CHUNK_*.md ELEVATION_DATUM_ISSUE.md archive/session-notes/ 2>/dev/null || true
mv REFERENCE_IMAGES_OLD.md archive/session-notes/ 2>/dev/null || true

# Move test scripts
echo "Moving test scripts..."
mv *.sh archive/test-scripts/ 2>/dev/null || true
mv test_*.rs check_*.rs debug_*.rs archive/test-scripts/ 2>/dev/null || true
mv *.py archive/test-scripts/ 2>/dev/null || true

# Move important docs to docs/
echo "Moving architecture docs to docs/..."
mv ARCHITECTURE_VIOLATION.md docs/ 2>/dev/null || true
mv ASYNC_*.md docs/ 2>/dev/null || true
mv BRIDGE_TUNNEL_SYSTEM.md docs/ 2>/dev/null || true
mv CODE_QUALITY_FIXES.md docs/ 2>/dev/null || true
mv CONTINUOUS_QUERIES_VOLUMETRIC_FIX.md docs/ 2>/dev/null || true
mv LOD_HYSTERESIS_COMPLETE.md docs/ 2>/dev/null || true
mv ORGANIC_TERRAIN_RESEARCH.md docs/ 2>/dev/null || true
mv VOLUMETRIC_ARCHITECTURE_REQUIRED.md docs/ 2>/dev/null || true
mv WHAT_I_ACTUALLY_LEARNED.md docs/ 2>/dev/null || true

# Move temp analysis
echo "Moving temporary analysis files..."
mv RUN_*.md TEST_*.md archive/temp-analysis/ 2>/dev/null || true
mv TERRAIN_FIX_PHASE1.md TERRAIN_QUALITY_ANALYSIS.md archive/temp-analysis/ 2>/dev/null || true
mv SVO_CHUNK_ARCHITECTURE.md archive/temp-analysis/ 2>/dev/null || true

# Remove test_clone if exists
echo "Removing test directories..."
rm -rf test_clone 2>/dev/null || true

# Restore this script to archive
mv do-cleanup.sh archive/test-scripts/ 2>/dev/null || true

echo ""
echo "✓ Cleanup complete"
echo ""
echo "Remaining in root:"
ls -1 *.md *.sh *.rs 2>/dev/null | wc -l
