SCALE_NOTE = "calibration multiplier for raw boarding_count readings"


def scaled_boarding_count(value):
    # Scale a raw boarding_count reading by the calibration factor.
    return value * 1.25
