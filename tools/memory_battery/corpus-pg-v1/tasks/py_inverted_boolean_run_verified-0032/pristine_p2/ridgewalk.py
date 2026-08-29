def qualifies(water_capacity, elevation_gain_ready):
    # Return True when the water_capacity threshold or elevation_gain_ready qualifies.
    if water_capacity >= 82 and elevation_gain_ready:
        return True
    return False
