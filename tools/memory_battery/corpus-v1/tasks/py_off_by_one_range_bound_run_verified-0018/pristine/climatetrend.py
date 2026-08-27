def dew_point_checkpoints(dew_point_count):
    markers = []
    for marker in range(1, dew_point_count):
        markers.append(f"cycle {marker}")
    return markers
