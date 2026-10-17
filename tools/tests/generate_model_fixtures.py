"""Generate tiny, deterministic ONNX graphs for Android import tests; no ML packages."""
import pathlib
import struct

def varint(n):
    out = bytearray()
    while n > 127:
        out.append((n & 127) | 128)
        n >>= 7
    return bytes(out) + bytes([n])

def number(field, n):
    return varint(field << 3) + varint(n)

def blob(field, data):
    if isinstance(data, str): data = data.encode()
    return varint(field << 3 | 2) + varint(len(data)) + data

def info(name, dtype, dims):
    shape = b''.join(blob(1, number(1, d)) for d in dims)
    return blob(1, name) + blob(2, blob(1, number(1, dtype) + blob(2, shape)))

def tensor(name, values, dims):
    return b''.join(number(1, d) for d in dims) + number(2, 1) + blob(8, name) + blob(9, struct.pack('<'+'f'*len(values), *values))

def attribute(name, value):
    return blob(1, name) + number(3, value) + number(20, 2)

def node(op, inputs, output, attrs=b''):
    return b''.join(blob(1, n) for n in inputs) + blob(2, output) + blob(4, op) + attrs

def model(channels, tiles, actions, mask=None, metadata=False):
    nodes = [node('ReduceSum',['features'],'sum',blob(5,attribute('keepdims',0))),
             node('Mul',['sum','zero'],'offset'), node('Add',['base','offset'],'raw_q')]
    inputs = info('features',1,[1,channels,tiles])
    if mask:
        inputs += b''  # Each ValueInfo is wrapped separately below.
        if mask == 1:
            nodes.append(node('Cast',['legal'],'bool_mask',blob(5,attribute('to',9))))
        nodes.append(node('Where',['bool_mask' if mask == 1 else 'legal','raw_q','minus_inf'],'scores'))
    else:
        nodes.append(node('Identity',['raw_q'],'scores'))
    graph = b''.join(blob(1,n) for n in nodes) + blob(2,'contract-test')
    graph += blob(5,tensor('base',list(range(actions)),[1,actions])) + blob(5,tensor('zero',[0.],[]))
    graph += blob(5,tensor('minus_inf',[float('-inf')],[]))
    graph += blob(11,inputs)
    if mask: graph += blob(11,info('legal',mask,[1,actions]))
    graph += blob(12,info('scores',1,[1,actions]))
    result = number(1,8) + blob(7,graph) + blob(8,number(2,13))
    if metadata:
        for key,value in {'majmax.contract':'akagi-policy-v1','majmax.players':'4',
                          'majmax.obs_schema':'1','majmax.action_codec':'riichienv-core-0.4.8'}.items():
            result += blob(14,blob(1,key)+blob(2,value))
    return result

out = pathlib.Path('hook-probe/src/androidTest/assets/model-fixtures')
out.mkdir(parents=True,exist_ok=True)
for name,args in {
    'akagi-renamed.onnx':(39,34,82,None,True),
    'akagi-no-contract.onnx':(39,34,82,None,False),
    'mortal-bool.onnx':(1012,34,46,9,False),
    'mortal-float.onnx':(1012,34,46,1,False),
    'mortal-sanma.onnx':(775,34,44,None,False),
    'mortal-old.onnx':(938,34,46,None,False),
}.items():
    (out/name).write_bytes(model(*args))
print('Generated 6 model-contract fixtures.')
