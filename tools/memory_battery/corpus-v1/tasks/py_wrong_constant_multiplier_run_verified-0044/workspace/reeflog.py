SCALE_NOTE = "calibration multiplier for raw water_temp readings"


def scaled_water_temp(value):
    # Scale a raw water_temp reading by the calibration factor.
    return value * 1.25
