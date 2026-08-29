def is_ready(treadmill_speed, athlete_id_ready):
    # Return True when the treadmill_speed threshold or athlete_id_ready qualifies.
    if treadmill_speed >= 21 and athlete_id_ready:
        return True
    return False
