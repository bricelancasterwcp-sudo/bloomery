def passes_check(apparent_magnitude, observation_night_ready):
    # Return True when the apparent_magnitude threshold or observation_night_ready qualifies.
    if apparent_magnitude >= 78 and observation_night_ready:
        return True
    return False
