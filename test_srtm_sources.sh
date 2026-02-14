#!/bin/bash
# Test SRTM sources for Brisbane (S28E153 tile)

echo "Testing SRTM data sources for S28E153 (Brisbane)..."
echo ""

# Source 1: OpenTopography (NASA SRTM via API - requires auth)
echo "1. OpenTopography: Requires API key, skipping"

# Source 2: USGS EarthExplorer (requires login)
echo "2. USGS EarthExplorer: Requires login, skipping"

# Source 3: CGIAR-CSI Mirror (5x5 degree tiles)
echo "3. CGIAR-CSI 5x5 tiles:"
curl -s -I "https://srtm.csi.cgiar.org/wp-content/uploads/files/srtm_5x5/SRTM_Data_ArcASCII/srtm_66_20.zip" | head -n 1

# Source 4: Viewfinder Panoramas (1 arc-second DEMs)
echo "4. Viewfinder Panoramas:"
curl -s -I "http://viewfinderpanoramas.org/dem3/S28.zip" | head -n 1

# Source 5: SRTM Kurviger (3 arc-second)
echo "5. SRTM Kurviger:"
curl -s -I "https://srtm.kurviger.de/SRTM3/S28E153.hgt.zip" | head -n 1

# Source 6: NASA EARTHDATA (direct, requires auth)
echo "6. NASA EARTHDATA: Requires auth, skipping"

# Source 7: JAXA Global (30m, free but requires registration)
echo "7. JAXA AW3D30: Requires registration, skipping"

echo ""
echo "Testing complete. HTTP 200 = working source"
