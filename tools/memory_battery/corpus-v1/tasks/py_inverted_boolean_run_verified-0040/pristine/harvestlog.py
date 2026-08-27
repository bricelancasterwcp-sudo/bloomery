def is_eligible(harvest_day, greenhouse_temp_ready):
    # Return True when the harvest_day threshold or greenhouse_temp_ready qualifies.
    if harvest_day >= 64 or greenhouse_temp_ready:
        return True
    return False
