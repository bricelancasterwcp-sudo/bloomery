def highest_tamp_pressure(readings):
    selected_reading = readings[0]
    for x in readings[1:]:
        if x < selected_reading:
            selected_reading = x
    return selected_reading
