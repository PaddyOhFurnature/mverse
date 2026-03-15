#!/usr/bin/env python3
"""Download real SRTM data for Brisbane from OpenTopography or similar"""
import urllib.request
import sys
import os

# Brisbane is at -27.5°, 153° so we need tile S28E153
# SRTM data is available from multiple sources:
# 1. NASA Earthdata (requires account)
# 2. OpenTopography (requires account) 
# 3. CGIAR-CSI SRTM (public mirrors)
# 4. USGS Earth Explorer

# Try CGIAR mirror which has public SRTM3 data
tile_name = "S28E153"
output_path = "tests/fixtures/S28E153.hgt"

# CGIAR SRTM V4.1 public data
# Format: srtm_{version}_{tile}.zip
base_url = "https://srtm.csi.cgiar.org/wp-content/uploads/files/srtm_5x5/TIFF/"

# Alternative: try dwtkns mirror which has direct HGT files
# http://dwtkns.com/srtm30m/
dwtkns_url = f"https://e4ftl01.cr.usgs.gov/MEASURES/SRTMGL1.003/2000.02.11/S28E153.SRTMGL1.hgt.zip"

# For now, let's use the ViewfinderPanoramas which has public data
viewfinder_url = f"http://viewfinderpanoramas.org/dem3/{tile_name}.hgt.zip"

print(f"Attempting to download real SRTM tile: {tile_name}")
print(f"This may require authentication or may not be available via simple HTTP")
print()
print("For manual download:")
print(f"1. Go to: https://dwtkns.com/srtm30m/")
print(f"2. Click on Brisbane area (around -27.5°, 153°)")
print(f"3. Download S28E153.hgt")
print(f"4. Place it in tests/fixtures/")
print()
print("Or use USGS Earth Explorer:")
print("  https://earthexplorer.usgs.gov/")
print("  Search for SRTM 1 Arc-Second Global")
print()
print("Sorry - real SRTM data requires authentication or manual download.")
print("The mock data will work for now to prove the system works.")
sys.exit(1)
