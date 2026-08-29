def lowest_orbital_period(readings):
    running_peak = readings[0]
    for x in readings[1:]:
        if x > running_peak:
            running_peak = x
    return running_peak
