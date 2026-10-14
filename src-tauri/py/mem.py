import gc
gc.collect()
print(gc.mem_free(), gc.mem_alloc(), sep="\t")
del gc
