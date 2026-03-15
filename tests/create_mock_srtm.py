#!/usr/bin/env python3
"""Create a mock SRTM tile for Brisbane (S28E153)"""
import struct
import math

# Brisbane is at approximately -27.5°, 153.0°
# SRTM tiles are 1°x1° starting at SW corner
# S28E153 covers: -28° to -27° latitude, 153° to 154° longitude

# SRTM3 resolution: 1201x1201 samples (3 arc-seconds = ~90m)
SIZE = 1201

# Mt Coot-tha is at approximately -27.48°, 152.96° with elevation ~287m
# Most of Brisbane CBD is at sea level to 50m

def get_elevation(row, col):
    """Calculate elevation for a given sample position"""
    # Row 0 = north edge (-27°), Row 1200 = south edge (-28°)
    # Col 0 = west edge (153°), Col 1200 = east edge (154°)
    
    lat = -27.0 - (row / 1200.0)  # -27.0 to -28.0
    lon = 153.0 + (col / 1200.0)  # 153.0 to 154.0
    
    # Create a simple elevation model:
    # - Base elevation: 10m (coastal plain)
    # - Add a hill at Mt Coot-tha location (-27.48, 152.96)
    #   Note: 152.96 is west of tile boundary, so hill won't show
    # - Add a gentle hill in the center for testing
    
    base_elev = 10.0
    
    # Create a test hill in the western part of the tile
    # Center it around -27.5°, 153.2° 
    hill_lat = -27.5
    hill_lon = 153.2
    
    # Distance from hill center (in degrees)
    dist = math.sqrt((lat - hill_lat)**2 + (lon - hill_lon)**2)
    
    # Gaussian hill with peak of 200m, radius of 0.1° (~11km)
    if dist < 0.2:
        hill_height = 200.0 * math.exp(-(dist**2) / (2 * 0.05**2))
        return int(base_elev + hill_height)
    
    return int(base_elev)

# Create HGT file
output = bytearray()

for row in range(SIZE):
    for col in range(SIZE):
        elev = get_elevation(row, col)
        # Clamp to valid i16 range
        elev = max(-32767, min(32767, elev))
        # Write as big-endian signed 16-bit integer
        output.extend(struct.pack('>h', elev))

# Write to file
with open('tests/fixtures/S28E153.hgt', 'wb') as f:
    f.write(output)

print(f"Created S28E153.hgt ({len(output)} bytes)")
print(f"Coverage: -28° to -27° lat, 153° to 154° lon")
print(f"Resolution: {SIZE}x{SIZE} samples (SRTM3)")
print(f"Test hill at -27.5°, 153.2° with peak ~210m elevation")
