"""Writes examples/blocks.schem: one row of each tricky block type (transparency, models, redstone).

Usage: python3 examples/make_blocks.py examples/blocks.schem
"""
import gzip, struct, sys
def s(x): b=x.encode(); return struct.pack('>H',len(b))+b
def tag(t,name,payload): return bytes([t])+s(name)+payload
def short(n,v): return tag(2,n,struct.pack('>h',v))
def int_(n,v): return tag(3,n,struct.pack('>i',v))
def varint(v):
    out=b''
    while True:
        b=v&0x7f; v>>=7
        if v: out+=bytes([b|0x80])
        else: return out+bytes([b])
W,H,L=16,5,12
blocks={}
def put(x,y,z,st): blocks[(x,y,z)]=st
for x in range(W):
    for z in range(L): put(x,0,z,'minecraft:stone_bricks')
row=[ 'glass','white_stained_glass','red_stained_glass','oak_leaves','slime_block','honey_block','ice','tinted_glass','water','lava',
      'oak_planks','grass_block','oak_log[axis=x]','oak_log[axis=z]','observer[facing=east]','crafting_table']
for i,b in enumerate(row): put(i,1,1,'minecraft:'+b)
put(0,1,3,'minecraft:oak_stairs[facing=north,half=bottom,shape=straight]')
put(1,1,3,'minecraft:oak_stairs[facing=east,half=bottom,shape=straight]')
put(2,1,3,'minecraft:oak_stairs[facing=south,half=top,shape=straight]')
put(3,1,3,'minecraft:oak_stairs[facing=west,half=bottom,shape=outer_left]')
put(4,1,3,'minecraft:piston[facing=east,extended=true]'); put(5,1,3,'minecraft:piston_head[facing=east,type=normal,short=false]')
put(6,1,3,'minecraft:sticky_piston[facing=up,extended=false]')
put(7,1,3,'minecraft:sticky_piston[facing=south,extended=false]')
put(8,1,3,'minecraft:torch'); put(9,1,3,'minecraft:wall_torch[facing=east]'); put(9,2,3,'minecraft:redstone_torch[lit=true]')
put(10,1,3,'minecraft:redstone_wire[power=15,north=side,south=side,east=none,west=none]')
put(10,1,4,'minecraft:redstone_wire[power=0,north=side,south=none,east=side,west=none]')
put(11,1,3,'minecraft:repeater[facing=north,delay=3,powered=true]')
put(12,1,3,'minecraft:comparator[facing=west,mode=subtract,powered=false]')
put(13,1,3,'minecraft:lever[face=floor,facing=north,powered=true]')
put(14,1,3,'minecraft:oak_slab[type=bottom,waterlogged=true]')
put(15,1,3,'minecraft:hopper[facing=down]')
for z in (6,7,8):
    put(0,1,z,'minecraft:oak_fence[north=%s,south=%s,east=false,west=false]'%(str(z>6).lower(),str(z<8).lower()))
    put(2,1,z,'minecraft:glass_pane[north=%s,south=%s,east=false,west=false]'%(str(z>6).lower(),str(z<8).lower()))
    put(4,1,z,'minecraft:cobblestone_wall[north=%s,south=%s,east=none,west=none,up=true]'%(('low' if z>6 else 'none'),('low' if z<8 else 'none')))
    put(6,1,z,'minecraft:iron_bars[north=%s,south=%s,east=false,west=false]'%(str(z>6).lower(),str(z<8).lower()))
put(8,1,6,'minecraft:oak_door[facing=south,half=lower,hinge=left,open=false]'); put(8,2,6,'minecraft:oak_door[facing=south,half=upper,hinge=left,open=false]')
put(9,1,6,'minecraft:iron_door[facing=south,half=lower,hinge=left,open=true]'); put(9,2,6,'minecraft:iron_door[facing=south,half=upper,hinge=left,open=true]')
put(10,1,6,'minecraft:oak_trapdoor[facing=north,half=bottom,open=true]')
put(11,1,6,'minecraft:chest[facing=south]'); put(12,1,6,'minecraft:lantern[hanging=false]'); put(13,1,6,'minecraft:short_grass'); put(14,1,6,'minecraft:poppy')
put(15,1,6,'minecraft:oak_sign[rotation=0]')
put(8,1,9,'minecraft:rail[shape=north_south]'); put(9,1,9,'minecraft:stone_button[face=wall,facing=north]'); put(10,1,9,'minecraft:target')
put(11,1,9,'minecraft:white_bed[facing=south,part=foot]'); put(12,1,9,'minecraft:scaffolding'); put(13,1,9,'minecraft:daylight_detector'); put(14,1,9,'minecraft:note_block')
pal={'minecraft:air':0}
data=b''
for y in range(H):
    for z in range(L):
        for x in range(W):
            st=blocks.get((x,y,z),'minecraft:air')
            if st not in pal: pal[st]=len(pal)
            data+=varint(pal[st])
palp=b''.join(int_(k,v) for k,v in pal.items())+b'\0'
body=short('Width',W)+short('Height',H)+short('Length',L)+int_('Version',2)+int_('PaletteMax',len(pal))+tag(10,'Palette',palp)+tag(7,'BlockData',struct.pack('>i',len(data))+data)+b'\0'
open(sys.argv[1],'wb').write(gzip.compress(tag(10,'Schematic',body)))
