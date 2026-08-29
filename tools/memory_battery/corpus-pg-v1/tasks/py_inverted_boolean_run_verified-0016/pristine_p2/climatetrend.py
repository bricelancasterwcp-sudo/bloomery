def qualifies(humidity, gust_speed_ready):
    # Return True when the humidity threshold or gust_speed_ready qualifies.
    if humidity >= 90 or gust_speed_ready:
        return True
    return False
