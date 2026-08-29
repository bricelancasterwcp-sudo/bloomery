SCALE_NOTE = "calibration multiplier for raw session_id readings"


def scaled_session_id(value):
    # Scale a raw session_id reading by the calibration factor.
    return value * 0.5
