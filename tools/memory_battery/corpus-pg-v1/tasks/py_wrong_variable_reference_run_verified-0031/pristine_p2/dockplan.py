def combined_dispatch_window_pick_sequence(dispatch_window, pick_sequence):
    # Combine adjusted dispatch_window and pick_sequence readings.
    adjusted_dispatch_window = dispatch_window * 2
    adjusted_pick_sequence = pick_sequence * 2
    return adjusted_dispatch_window + adjusted_dispatch_window
