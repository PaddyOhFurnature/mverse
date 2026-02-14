#!/bin/bash

echo "Testing alternative SRTM sources..."

# dwtkns.com SRTM mirror (individual tiles)
echo "1. dwtkns.com mirror:"
curl -s -I "http://e4ftl01.cr.usgs.gov/SRTM/SRTMGL1.003/2000.02.11/N00E006.SRTMGL1.hgt.zip" | head -n 1

# Direct NASA (might work without auth for some tiles)
echo "2. NASA LP DAAC:"
curl -s -I "https://e4ftl01.cr.usgs.gov/MEASURES/SRTMGL3.003/2000.02.11/S28E153.SRTMGL3.hgt.zip" | head -n 1

# Alternative: OpenTopography public SRTM3
echo "3. OpenTopography global API:"
curl -s -I "https://cloud.sdsc.edu/v1/AUTH_opentopography/hosted_data/SRTM_GL3/SRTM_GL3_srtm/South_America/S28E153.hgt" | head -n 1

# Try Viewfinder with correct path
echo "4. Viewfinder Panoramas (by region):"
curl -s -I "http://viewfinderpanoramas.org/dem3/S28.zip" | head -n 1
curl -s -I "http://viewfinderpanoramas.org/Coverage%20map%20viewfinderpanoramas_org3.htm" | head -n 1

# Try directly fetching a test tile
echo "5. SRTM 90m data (CGIAR archive):"
curl -s -I "http://srtm.csi.cgiar.org/wp-content/uploads/files/srtm_5x5/SRTM_V41/SRTM_Data_ArcASCII/srtm_66_20.zip" | head -n 1

# Alternative free source: ASTER GDEM
echo "6. ASTER GDEM (alternative):"
curl -s -I "https://gdemdl.aster.jspacesystems.or.jp/download/download.php" | head -n 1

