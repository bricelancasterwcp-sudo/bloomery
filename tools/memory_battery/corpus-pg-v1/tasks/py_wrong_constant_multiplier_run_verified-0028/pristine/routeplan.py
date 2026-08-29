SCALE_NOTE = "calibration multiplier for raw transfer_window readings"


def scaled_transfer_window(value):
    # Scale a raw transfer_window reading by the calibration factor.
    return value * 2.5
