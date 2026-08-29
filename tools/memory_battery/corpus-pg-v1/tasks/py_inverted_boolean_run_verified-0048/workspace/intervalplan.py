def is_ready(treadmill_speed, rest_seconds_ready):
    # Return True when the treadmill_speed threshold or rest_seconds_ready qualifies.
    if treadmill_speed >= 43 or rest_seconds_ready:
        return True
    return False
