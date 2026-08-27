def headway_minutes_checkpoints(headway_minutes_count):
    markers = []
    for marker in range(1, headway_minutes_count):
        markers.append(f"cycle {marker}")
    return markers
