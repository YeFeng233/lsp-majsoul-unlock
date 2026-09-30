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
                val gate = AutoDiscardController({}, { time })
                gate.invalidate("test reset")
                run(gate) { time += 6_001 }
                tests++
                sendStatus(0, Bundle().apply { putString("stream", "PASS $name\n") })
            }
            fun live(revision: Int = 1, kind: String = "discard") = JSONObject("""
                {"status":"live","canAct":true,"connection":"7","revision":$revision,
                 "sourceGeneration":3,"sourceSequence":8,"hand":["5mr"],
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
            test("riichi recommendation remains manual") { gate, _ ->
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
            output.putString("stream", "$tests controller checks passed\n")
            finish(-1, output)
        }.onFailure {
            output.putString("stream", it.stackTraceToString()); finish(1, output)
        }
    }
}
