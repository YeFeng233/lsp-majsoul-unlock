"""Generate synthetic post-call/late-wall cases for the independent sanma audit."""
import argparse
import copy
import itertools
import json
from pathlib import Path

from validate_mortal_sanma import scenarios


def cases():
    base = {c['id']: c for c in scenarios(0)}
    result = []
    def add(name, source, tail, truncate=None):
        case = copy.deepcopy(base[source])
        case.update(id=name, group='extended')
        if truncate is not None: case['events'] = case['events'][:truncate]
        case['events'].extend(tail)
        result.append(case)
    add('after-own-kita', 'kita', [dict(type='nukidora', actor=0, pai='N'), dict(type='tsumo', actor=0, pai='7p')])
    add('after-opponent-kita', 'kita', [dict(type='tsumo', actor=1, pai='?'),
        dict(type='nukidora', actor=1, pai='N'),dict(type='tsumo', actor=1, pai='?'),
        dict(type='dahai', actor=1, pai='1m', tsumogiri=True),dict(type='tsumo', actor=2, pai='?'),
        dict(type='dahai', actor=2, pai='9m', tsumogiri=True),dict(type='tsumo', actor=0, pai='7p')],2)
    add('immediately-after-pon', 'pon', [dict(type='pon', actor=0,target=1,pai='P',consumed=['P','P'])])
    add('draw-after-pon', 'kakan', [])
    add('after-own-ankan', 'ankan', [dict(type='ankan',actor=0,consumed=['1p']*4),
        dict(type='dora',dora_marker='9m'),dict(type='tsumo',actor=0,pai='7p')])
    add('after-own-daiminkan', 'ankan', [dict(type='tsumo',actor=1,pai='?'),
        dict(type='dahai',actor=1,pai='1p',tsumogiri=True),
        dict(type='daiminkan',actor=0,target=1,pai='1p',consumed=['1p']*3),
        dict(type='dora',dora_marker='9m'),dict(type='tsumo',actor=0,pai='7p')],2)
    add('after-own-kakan', 'kakan', [dict(type='kakan',actor=0,pai='P',consumed=['P']*3),
        dict(type='dora',dora_marker='9m'),dict(type='tsumo',actor=0,pai='7p')])
    add('opponent-pon', 'kita', [dict(type='tsumo',actor=1,pai='?'),
        dict(type='dahai',actor=1,pai='5p',tsumogiri=True),
        dict(type='pon',actor=2,target=1,pai='5p',consumed=['5pr','5p']),
        dict(type='dahai',actor=2,pai='1m',tsumogiri=False),dict(type='tsumo',actor=0,pai='7p')],2)
    add('riichi-discard-phase', 'own-riichi', [],4)
    add('riichi-ippatsu-tsumo', 'own-riichi', [dict(type='tsumo',actor=0,pai='E')],-1)
    add('rinshan-tsumo-after-kita', 'tsumo', [dict(type='nukidora',actor=0,pai='N'),dict(type='tsumo',actor=0,pai='E')])
    result[-1]['events'][1]['tehais'][0][-1]='N'
    add('riichi-kita-ippatsu-tsumo', 'own-riichi', [dict(type='nukidora',actor=0,pai='N'),dict(type='tsumo',actor=0,pai='E')])
    add('opponent-kita-during-ippatsu', 'own-riichi', [dict(type='tsumo',actor=1,pai='?'),
        dict(type='nukidora',actor=1,pai='N'),dict(type='tsumo',actor=1,pai='?'),
        dict(type='dahai',actor=1,pai='1m',tsumogiri=True),dict(type='tsumo',actor=2,pai='?'),
        dict(type='dahai',actor=2,pai='9m',tsumogiri=True),dict(type='tsumo',actor=0,pai='E')],6)
    # Keep the same visible hand but vary all seat rotations and score orders.
    template = copy.deepcopy(base['kita'])
    for scores in itertools.permutations([10000,30000,65000]):
        for seat in range(3):
            case=copy.deepcopy(template);case.update(id=f'rank-{scores[0]}-{scores[1]}-s{seat}',group='rank',seat=seat)
            start=case['events'][1];hand=start['tehais'][0]
            start['tehais']=[hand if i==seat else ['?']*13 for i in range(4)]
            start['scores']=list(scores)+[0];start['oya']=(seat+1)%3
            case['events'][-1]['actor']=seat;result.append(case)
    late=copy.deepcopy(template);late.update(id='exhausted-live-wall',group='late-wall')
    late['events']=late['events'][:2]
    wall=[t for t in ['1m','9m']+[f'{i}{s}' for s in 'ps' for i in range(1,10)]+list('ESWNPFC') for _ in range(4)]
    for t in late['events'][1]['tehais'][0]+['9s']: wall.remove(t)
    for draw in range(55):
        who=draw%3;t=wall[draw]
        late['events'].append(dict(type='tsumo',actor=who,pai=t if who==0 else '?'))
        if draw<54:late['events'].append(dict(type='dahai',actor=who,pai=t,tsumogiri=True))
    result.append(late)
    result.extend(copy.deepcopy(list(base.values())))
    result.extend(c for c in scenarios(43) if c['id'] in ['random-003','random-042'])
    return result


if __name__ == '__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('output',type=Path);a=p.parse_args()
    data=cases();a.output.write_text(json.dumps(data),encoding='utf-8');print('Exported',len(data),'cases')
