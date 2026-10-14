# Prepare an OTA update: the partition after the running one. Prints its
# label, size and block size.
def _mb_ota_begin():
    import esp32
    p = esp32.Partition(esp32.Partition.RUNNING).get_next_update()
    i = p.info()
    print(i[4], i[3], p.ioctl(5, 0))
    return p


_mb_ota = _mb_ota_begin()
del _mb_ota_begin
