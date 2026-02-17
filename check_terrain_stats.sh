#!/bin/bash
# Analyze terrain generation parameters

echo "=== CURRENT TERRAIN GENERATION SETTINGS ==="
echo ""
echo "Voxel Resolution:"
grep -n "VOXEL_SIZE_M" src/procedural_generator.rs

echo ""
echo "Surface Layer Thickness:"
grep -n "dist_from_surface >=" src/procedural_generator.rs | head -5

echo ""
echo "Block Size:"
grep -n "BLOCK_SIZE_M\|VOXELS_PER_BLOCK" src/procedural_generator.rs | head -5

echo ""
echo "=== ANALYSIS ==="
echo ""
echo "Current surface layer: -4m to +2m = 6 meters thick"
echo "Kangaroo Point Cliffs: ~20-30m tall vertical drop"
echo ""
echo "Problem: Surface layer only captures 6m of a 30m cliff!"
echo "Solution: Increase to at least -20m to +10m (30m range)"
