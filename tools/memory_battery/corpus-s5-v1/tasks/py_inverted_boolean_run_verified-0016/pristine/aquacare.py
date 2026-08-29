def qualifies(tank_volume, salinity_ready):
    # Return True when the tank_volume threshold or salinity_ready qualifies.
    if tank_volume >= 48 and salinity_ready:
        return True
    return False
