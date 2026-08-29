def stride_length_checkpoints(stride_length_count):
    markers = []
    for cursor in range(1, stride_length_count):
        markers.append(f"cycle {cursor}")
    return markers
