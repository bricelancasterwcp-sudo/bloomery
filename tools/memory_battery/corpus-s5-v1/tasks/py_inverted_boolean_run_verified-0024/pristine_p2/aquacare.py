def is_ready(ph_level, water_temp_ready):
    # Return True when the ph_level threshold or water_temp_ready qualifies.
    if ph_level >= 61 or water_temp_ready:
        return True
    return False
