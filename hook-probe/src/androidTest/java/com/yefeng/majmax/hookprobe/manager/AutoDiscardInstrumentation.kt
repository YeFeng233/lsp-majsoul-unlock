package com.yefeng.majmax.hookprobe.manager

import android.app.Instrumentation
import android.os.Bundle
import org.json.JSONArray
import org.json.JSONObject

/** Runs against Android's real JSONObject, clock supplied explicitly; no game input. */
class AutoDiscardInstrumentation : Instrumentation() {
    override fun onCreate(arguments: Bundle?) { super.onCreate(arguments); start() }
    override fun onStart() {
        val output = Bundle()
        runCatching {
            var tests = 0
            fun test(name: String, run: (AutoDiscardController, () -> Unit) -> Unit) {
                var time = 0L
                val gate = AutoDiscardController({}, { time }, { 0L })
                gate.invalidate("test reset")
                run(gate) { time += 12_001 }
                tests++
                sendStatus(0, Bundle().apply { putString("stream", "PASS $name\n") })
            }
            fun live(revision: Int = 1, kind: String = "discard") = JSONObject("""
                {"status":"live","canAct":true,"connection":"7","revision":$revision,
                 "sourceGeneration":3,"sourceSequence":8,"seat":0,"players":4,"hand":["5mr"],
                 "legalOperations":[{"type":1,"combination":[]}],
                 "autoContext":{"riichi":false},
                 "recommendations":[{"kind":"$kind","tile":"5mr","tsumogiri":false}]}
            """.trimIndent())
            fun ack(id: String, state: String) = JSONArray().put(JSONObject().put("id",id).put("state",state)).toString()
            test("default off and explicit enable") { gate, _ ->
                gate.observe(live()); check(gate.poll(null) == null)
                gate.toggle(); val command = JSONObject(checkNotNull(gate.poll(null)))
                check(command.getString("gameTile") == "0m")
                check(!command.getBoolean("tsumogiri"))
            }
            test("demo and disconnected sessions cannot enable") { gate, _ ->
                gate.observe(live().put("status","demo")); gate.toggle(); check(gate.poll(null) == null)
                gate.observe(live()); gate.invalidate("disconnected"); gate.toggle(); check(gate.poll(null) == null)
            }
            test("new capture session invalidates queued input") { gate, _ ->
                gate.observe(live()); gate.toggle(); check(gate.poll(null) != null)
                gate.invalidate("new session"); check(gate.poll(null) == null && !AutoDiscardState.mutable.value.enabled)
            }
            test("manual operation pauses") { gate, _ ->
                gate.observe(live()); gate.toggle()
                gate.poll(ack("", "manual")); check(gate.poll(null) == null && !AutoDiscardState.mutable.value.enabled)
            }
            test("manual input while disabled does not alter paused state") { gate, _ ->
                gate.observe(live())
                val previous = AutoDiscardState.mutable.value
                gate.poll(ack("", "manual"))
                check(previous == AutoDiscardState.mutable.value && gate.poll(null) == null)
            }
            test("model failure pauses") { gate, _ ->
                gate.observe(live()); gate.toggle(); gate.observe(live().put("modelFallback",true))
                check(gate.poll(null) == null && !AutoDiscardState.mutable.value.enabled)
            }
            test("riichi requires a server declaration prompt") { gate, _ ->
                gate.observe(live(kind="riichi")); gate.toggle(); check(gate.poll(null) == null)
            }
            test("timeout pauses without retry") { gate, advance ->
                gate.observe(live()); gate.toggle(); advance()
                check(gate.poll(null) == null && !AutoDiscardState.mutable.value.enabled)
            }
            test("stale rejection does not replay decision") { gate, _ ->
                gate.observe(live()); gate.toggle(); val id=JSONObject(gate.poll(null)!!).getString("id")
                gate.poll(ack(id,"stale")); gate.toggle(); check(gate.poll(null) == null)
            }
            test("sent input requires server acceptance before next decision") { gate, _ ->
                gate.observe(live()); gate.toggle(); val id=JSONObject(gate.poll(null)!!).getString("id")
                gate.poll(ack(id,"submitted"))
                gate.observe(live().put("canAct",false).put("input",JSONObject("""
                    {"method":".lq.FastTest.inputOperation","payload":{"type":1,"tile":"0m","moqie":false}}
                """)))
                gate.observe(live(2)); check(gate.poll(null) == null)
                gate.poll(ack(id,"accepted")); val next=JSONObject(checkNotNull(gate.poll(null)))
                check(next.getString("id") != id && next.getInt("revision") == 2)
            }
            test("unmatched uplink pauses") { gate, _ ->
                gate.observe(live()); gate.toggle()
                gate.observe(live().put("input",JSONObject("""
                    {"method":".lq.FastTest.inputOperation","payload":{"type":1,"tile":"5m","moqie":false}}
                """)))
                check(gate.poll(null) == null && !AutoDiscardState.mutable.value.enabled)
            }
            fun actionCase(kind: String, type: Int, tile: String = "", consumed: List<String> = emptyList(),
                           combo: List<String> = emptyList(), hand: List<String> = listOf("5mr"), target: Int = 3,
                           response: Boolean = true): JSONObject {
                val state=live(kind=kind).put("hand",JSONArray(hand))
                state.getJSONObject("autoContext").put("phase",if(response) "wait_response" else "wait_act").put("drawnTile","5mr")
                state.put("legalOperations",JSONArray().put(JSONObject().put("type",type).put("combination",JSONArray(combo))))
                state.put("recommendations",JSONArray().put(JSONObject().put("kind",kind).put("tile",tile)
                    .put("target",target).put("consumed",JSONArray(consumed))))
                return state
            }
            fun prepared(state: JSONObject) = checkNotNull(AutoDiscardController.prepareAction(state,state.getJSONArray("recommendations").getJSONObject(0)))
            test("riichi retains red tile and declaration type") { gate, _ ->
                val state=actionCase("riichi",7,"5mr",combo=listOf("5m"),response=false)
                gate.observe(state); gate.toggle()
                val cmd=JSONObject(checkNotNull(gate.poll(null)))
                check(cmd.getJSONObject("action").getInt("type")==7 && cmd.getBoolean("tsumogiri"))
                check(!AutoDiscardController.matchesInput(cmd,".lq.FastTest.inputOperation",JSONObject("""{"type":1,"tile":"0m","moqie":true}""")))
                check(AutoDiscardController.matchesInput(cmd,".lq.FastTest.inputOperation",JSONObject("""{"type":7,"tile":"0m","moqie":true}""")))
            }
            test("chi selects exact red combination index") { _, _ ->
                val state=actionCase("chi",2,"3m",listOf("4m","5mr"),listOf("4m|5m","0m|4m"),listOf("4m","5mr"))
                check(prepared(state).getInt("index")==1)
                state.getJSONArray("recommendations").getJSONObject(0).put("consumed",JSONArray(listOf("4m","5m")))
                check(AutoDiscardController.prepareAction(state,state.getJSONArray("recommendations").getJSONObject(0))==null)
            }
            test("pon and open kan retain target and consumed tiles") { _, _ ->
                for ((kind,type,n) in listOf(Triple("pon",3,2),Triple("kan",5,3))) {
                    val tiles=List(n){"5m"}; val state=actionCase(kind,type,"5m",tiles,listOf(List(n){"5m"}.joinToString("|")),tiles)
                    check(prepared(state).getInt("index")==0 && prepared(state).getInt("target")==3)
                    state.getJSONArray("recommendations").getJSONObject(0).put("target",0)
                    check(AutoDiscardController.prepareAction(state,state.getJSONArray("recommendations").getJSONObject(0))==null)
                }
            }
            test("ankan requires all four exact tiles") { _, _ ->
                val tiles=listOf("5m","5m","5m","5mr")
                check(prepared(actionCase("ankan",4,"5m",tiles,listOf("5m|0m|5m|5m"),tiles,response=false)).getInt("index")==0)
            }
            test("kakan accounts for red tile in the existing pon") { _, _ ->
                val state=actionCase("kakan",6,"5m",listOf("5m","5mr","5m"),listOf("5m|5m|0m|5m"),listOf("5m"),response=false)
                state.getJSONObject("autoContext").put("melds",JSONArray().put(JSONObject().put("kind","pon").put("tiles",JSONArray(listOf("5m","5mr","5m")))))
                check(prepared(state).getInt("index")==0)
            }
            test("ron and tsumo use distinct request methods") { _, _ ->
                check(prepared(actionCase("hora",9,target=3)).getString("method")=="inputChiPengGang")
                check(prepared(actionCase("hora",8,target=0,response=false)).getString("method")=="inputOperation")
            }
            test("pass sends an explicit cancel and never confirms a win") { _, _ ->
                val state=actionCase("pass",2)
                val cmd=JSONObject().put("action",prepared(state))
                check(AutoDiscardController.matchesInput(cmd,".lq.FastTest.inputChiPengGang",JSONObject().put("cancel_operation",true)))
                check(!AutoDiscardController.matchesInput(cmd,".lq.FastTest.inputChiPengGang",JSONObject().put("type",9)))
            }
            test("kita is sanma only and preserves moqie") { _, _ ->
                val state=actionCase("kita",11,"N",hand=listOf("N"),response=false)
                state.put("players",3); state.getJSONObject("autoContext").put("drawnTile","N")
                val cmd=JSONObject().put("action",prepared(state))
                check(AutoDiscardController.matchesInput(cmd,".lq.FastTest.inputOperation",JSONObject().put("type",11).put("moqie",true)))
                state.put("players",4)
                check(AutoDiscardController.prepareAction(state,state.getJSONArray("recommendations").getJSONObject(0))==null)
            }
            test("abort requires server nine terminals prompt") { _, _ ->
                check(prepared(actionCase("abort",10,response=false)).getInt("type")==10)
            }
            test("changed combination index cannot confirm another call") { gate, _ ->
                gate.observe(actionCase("chi",2,"3m",listOf("4m","5mr"),listOf("4m|5m","4m|0m"),listOf("4m","5mr"))); gate.toggle()
                gate.observe(live().put("input",JSONObject("""{"method":".lq.FastTest.inputChiPengGang","payload":{"type":2,"index":0}}""")))
                check(!AutoDiscardState.mutable.value.enabled)
            }
            test("riichi locks ordinary discard to drawn tile") { _, _ ->
                val state=live(); state.getJSONObject("autoContext").put("riichi",true)
                check(AutoDiscardController.prepareAction(state,state.getJSONArray("recommendations").getJSONObject(0))==null)
            }
            test("queued actions wait for their full random delay") { _, _ ->
                for (delay in listOf(2_000L,5_000L)) {
                    var time=0L
                    val gate=AutoDiscardController({}, {time}, {delay})
                    gate.observe(live()); gate.toggle(); check(gate.poll(null)==null)
                    time=delay-1; check(gate.poll(null)==null)
                    time=delay; check(JSONObject(checkNotNull(gate.poll(null))).getLong("delayMs")==delay)
                }
            }
            test("heartbeat renews sequence without restarting delay") { _, _ ->
                var time=0L
                val gate=AutoDiscardController({}, {time}, {2_000L})
                gate.observe(live()); gate.toggle()
                time=1_500; gate.observe(live().put("sourceSequence",9)); check(gate.poll(null)==null)
                time=2_000; check(JSONObject(checkNotNull(gate.poll(null))).getLong("sourceSequence")==9L)
            }
            test("changed decision during delay pauses") { _, _ ->
                var time=0L
                val gate=AutoDiscardController({}, {time}, {2_000L})
                gate.observe(live()); gate.toggle(); gate.observe(live(2))
                time=2_000; check(gate.poll(null)==null && !AutoDiscardState.mutable.value.enabled)
            }
            test("pause during delay never publishes input") { _, _ ->
                var time=0L
                val gate=AutoDiscardController({}, {time}, {5_000L})
                gate.observe(live()); gate.toggle(); gate.pause("manual")
                time=5_000; check(gate.poll(null)==null)
            }
            test("production random delay always stays within two to five seconds") { _, _ ->
                repeat(30) {
                    var time=0L
                    val gate=AutoDiscardController({}, {time})
                    gate.observe(live()); gate.toggle(); check(gate.poll(null)==null)
                    time=5_000
                    check(JSONObject(checkNotNull(gate.poll(null))).getLong("delayMs") in 2_000L..5_000L)
                }
            }
            output.putString("stream", "$tests controller checks passed\n")
            finish(-1, output)
        }.onFailure {
            output.putString("stream", it.stackTraceToString()); finish(1, output)
        }
    }
}
