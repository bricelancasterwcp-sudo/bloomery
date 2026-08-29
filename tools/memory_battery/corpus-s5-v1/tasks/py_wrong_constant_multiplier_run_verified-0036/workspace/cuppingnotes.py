SCALE_NOTE = "calibration multiplier for raw yield_grams readings"


def scaled_yield_grams(value):
    # Scale a raw yield_grams reading by the calibration factor.
    return value * 3.0
