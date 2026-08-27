def sample_rate_checkpoints(sample_rate_count):
    markers = []
    for cursor in range(1, sample_rate_count):
        markers.append(f"cycle {cursor}")
    return markers
