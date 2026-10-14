import gc, micropython
gc.collect()
micropython.mem_info(1)
del gc, micropython
