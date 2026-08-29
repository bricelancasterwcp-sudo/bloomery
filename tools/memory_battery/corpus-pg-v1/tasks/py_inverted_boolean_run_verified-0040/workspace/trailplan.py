def meets_criteria(blaze_spacing, summit_elevation_ready):
    # Return True when the blaze_spacing threshold or summit_elevation_ready qualifies.
    if blaze_spacing >= 77 and summit_elevation_ready:
        return True
    return False
