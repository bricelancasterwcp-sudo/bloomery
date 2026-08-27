def highest_soil_ph(readings):
    tracked_value = readings[0]
    for x in readings[1:]:
        if x < tracked_value:
            tracked_value = x
    return tracked_value
