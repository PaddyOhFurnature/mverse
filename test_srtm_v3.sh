#!/bin/bash

echo "Following redirects and testing HTTPS..."

# Viewfinder with https
echo "1. Viewfinder HTTPS:"
curl -s -L -I "https://viewfinderpanoramas.org/dem3/S28.zip" 2>&1 | grep -E "^HTTP|^Location" | head -n 3

# CGIAR with different path
echo -e "\n2. CGIAR HTTPS:"
curl -s -L -I "https://srtm.csi.cgiar.org/wp-content/uploads/files/srtm_5x5/SRTM_V41/SRTM_Data_GeoTiff/srtm_66_20.zip" 2>&1 | grep -E "^HTTP|^Location" | head -n 3

# Try USGS EarthExplorer anonymous
echo -e "\n3. USGS Anonymous:"
curl -s -I "https://dds.cr.usgs.gov/srtm/version2_1/SRTM3/Australia/S28E153.hgt.zip" 2>&1 | grep -E "^HTTP|^Location" | head -n 3

# AWS Terrain Tiles (Mapzen format, but elevation data)
echo -e "\n4. AWS Terrain Tiles:"
curl -s -I "https://s3.amazonaws.com/elevation-tiles-prod/geotiff/-28/153.tif" 2>&1 | grep -E "^HTTP|^Location" | head -n 1

# Try Japan's ALOS World 3D
echo -e "\n5. ALOS World 3D (requires registration, testing):"
curl -s -I "https://www.eorc.jaxa.jp/ALOS/en/dataset/aw3d30/aw3d30_e.htm" 2>&1 | grep -E "^HTTP" | head -n 1

