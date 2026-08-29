def lowest_heartrate_zone(readings):
    current_leader = readings[0]
    for x in readings[1:]:
        if x > current_leader:
            current_leader = x
    return current_leader
