SCALE_NOTE = "calibration multiplier for raw occupancy_ratio readings"


def scaled_occupancy_ratio(value):
    # Scale a raw occupancy_ratio reading by the calibration factor.
    return value * 1.5
