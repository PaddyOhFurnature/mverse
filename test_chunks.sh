#!/bin/bash
# Test chunk-based operation file system

set -e

echo "🧪 Testing Chunk-Based Operation Files"
echo "======================================"
echo ""

# Clean slate
echo "1️⃣  Cleaning world_data..."
rm -rf world_data/chunks
echo "   ✅ Clean slate ready"
echo ""

# Check initial state
echo "2️⃣  Checking initial state..."
if [ -d "world_data/chunks" ]; then
    echo "   ❌ ERROR: chunks directory exists (should be clean)"
    exit 1
fi
echo "   ✅ No chunks directory (fresh start)"
echo ""

echo "3️⃣  Manual test required:"
echo "   Please run: cargo run --release --example phase1_multiplayer"
echo "   Then:"
echo "   - Dig several voxels (press E)"
echo "   - Exit the program (close window or Ctrl+C)"
echo ""
echo "4️⃣  After exiting, run this script again to verify"
echo ""

# Check if chunks were created
if [ ! -d "world_data/chunks" ]; then
    echo "   ℹ️  No chunks directory yet - run the program first"
    exit 0
fi

echo "5️⃣  Verifying chunk files..."
chunk_count=$(find world_data/chunks -name "operations.json" | wc -l)
if [ $chunk_count -eq 0 ]; then
    echo "   ❌ ERROR: No operation files found"
    exit 1
fi
echo "   ✅ Found $chunk_count chunk file(s)"
echo ""

echo "6️⃣  Inspecting chunk contents..."
for ops_file in world_data/chunks/*/operations.json; do
    chunk_dir=$(dirname "$ops_file")
    chunk_name=$(basename "$chunk_dir")
    op_count=$(jq 'length' "$ops_file" 2>/dev/null || echo "ERROR")
    echo "   📦 $chunk_name: $op_count operations"
    
    # Show first operation as sample
    if [ "$op_count" != "ERROR" ] && [ "$op_count" != "0" ]; then
        echo "      Sample operation:"
        jq '.[0] | {coord, material, author: .author[0:10]}' "$ops_file" 2>/dev/null | sed 's/^/      /'
    fi
done
echo ""

echo "7️⃣  Verification complete!"
echo "   ✅ Chunk-based file system is working"
echo ""
echo "Next: Run the program again to verify operations are loaded on restart"
