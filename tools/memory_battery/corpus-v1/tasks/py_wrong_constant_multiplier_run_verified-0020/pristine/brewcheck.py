SCALE_NOTE = "calibration multiplier for raw extraction_time readings"


def scaled_extraction_time(value):
    # Scale a raw extraction_time reading by the calibration factor.
    return value * 2.5
