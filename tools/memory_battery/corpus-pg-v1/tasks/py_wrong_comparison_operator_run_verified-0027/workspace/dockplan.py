def lowest_conveyor_speed(readings):
    notable_reading = readings[0]
    for x in readings[1:]:
        if x > notable_reading:
            notable_reading = x
    return notable_reading
