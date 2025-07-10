use super::*;
use core::{array, ops::Not};

fn insert_success<N, I, O, Q, R>(graph: &mut Graph<N, I, O>, from: (N, O), to: (&Q, &R))
where
    N: Hash + Eq + borrow::Borrow<Q>,
    I: Hash + Eq + borrow::Borrow<R>,
    O: Hash + Eq,
    Q: ?Sized + Hash + Eq,
    R: ?Sized + Hash + Eq,
{
    assert!(graph.try_insert_edge_acyclic(from, to).unwrap())
}

// These tests aren't ideal, I have to print the compiled schedule and review it first,
// then insert it in the final assert directive if it's correct (TODO). This is inconvenient,
// since there are often many correct schedules, and any update to the traversal order used by the
// scheduler (optimizations, different hashing algorithms...) will break these tests, despite
// still creating correct schedules.

#[test]
fn basic_cycle() {
    let mut graph = Graph::default();

    let node1_id = "node1";
    let mut node1 = Node::default();
    let node1_input_id = "node1_input";
    let node1_output_id = "node1_output";
    node1.add_input(node1_input_id);
    node1.add_output(node1_output_id);

    graph.insert_node(node1_id, node1);

    assert!(
        graph
            .try_insert_edge_acyclic((node1_id, node1_output_id), (node1_id, node1_input_id))
            .is_err_and(|i| i)
    )
}

#[test]
fn insert_redundant_edge() {
    let mut graph = Graph::default();

    let mut node1 = Node::default();
    let node1_id = "node1";

    let node1_output_id = "node1_output";
    node1.add_output(node1_output_id);

    graph.insert_node(node1_id, node1);

    let mut node2 = Node::default();
    let node2_id = "node2";

    let node2_input_id = "node2_input";
    node2.add_input(node2_input_id);

    graph.insert_node(node2_id, node2);

    insert_success(
        &mut graph,
        (node1_id, node1_output_id),
        (node2_id, node2_input_id),
    );
    assert!(
        graph
            .try_insert_edge_acyclic((node1_id, node1_output_id), (node2_id, node2_input_id))
            .is_ok_and(Not::not)
    );
}

#[test]
fn basic() {
    let mut graph = Graph::default();

    let mut source = Node::default();
    let source_id = "source";
    let source_output_id = "source_output";
    source.add_output_with_latency(source_output_id, 5);
    graph.insert_node(source_id, source);

    let mut sink = Node::default();
    let sink_id = "sink";
    let sink_input_id = "sink_input";
    sink.add_input(sink_input_id);
    graph.insert_node(sink_id, sink);

    insert_success(
        &mut graph,
        (source_id, source_output_id),
        (sink_id, sink_input_id),
    );

    let mut scheduler = graph.scheduler();

    scheduler.add_sink_node(sink_id);

    let schedule = scheduler.compile();

    println!("{schedule:#?}");
    println!("{:#?}", scheduler.intermediate());
}

#[test]
fn chain() {
    let mut graph = Graph::default();

    let mut source = Node::default();
    let source_id = "source";
    let source_output_id = "source_output";
    source.add_output_with_latency(source_output_id, 4);
    graph.insert_node(source_id, source);

    let mut int1 = Node::default();
    let int1_id = "int1";
    let int1_output_id = "int1_output";
    let int1_input_id = "int1_input";
    int1.add_output_with_latency(int1_output_id, 6);
    int1.add_input(int1_input_id);
    graph.insert_node(int1_id, int1);

    let mut int2 = Node::default();
    let int2_id = "int2";
    let int2_output_id = "int2_output";
    let int2_input_id = "int2_input";
    int2.add_output_with_latency(int2_output_id, 9);
    int2.add_input(int2_input_id);
    graph.insert_node(int2_id, int2);

    let mut sink = Node::default();
    let sink_id = "sink";
    let sink_input_id = "sink_input";
    sink.add_input(sink_input_id);
    graph.insert_node(sink_id, sink);

    insert_success(
        &mut graph,
        (source_id, source_output_id),
        (int1_id, int1_input_id),
    );
    insert_success(
        &mut graph,
        (int1_id, int1_output_id),
        (int2_id, int2_input_id),
    );
    insert_success(
        &mut graph,
        (int2_id, int2_output_id),
        (sink_id, sink_input_id),
    );

    let mut scheduler = graph.scheduler();

    scheduler.add_sink_node(sink_id);

    let schedule = scheduler.compile();

    println!("{graph:#?}");
    println!("{schedule:#?}");
    println!("{:#?}", scheduler.intermediate());
}

#[test]
fn one_output_many_input_nodes() {
    let mut graph = Graph::default();

    let mut source = Node::default();
    let source_id = Box::<str>::from("source");
    let source_output_id = "source_output";
    source.add_output_with_latency(source_output_id, 10);
    graph.insert_node(source_id.clone(), source);

    const NUM_SINKS: usize = 4;

    for i in 0..NUM_SINKS {
        let mut sink = Node::default();

        let name = format!("sink{}", i + 1);

        let sink_id = name.clone().into_boxed_str();
        let sink_input_id = (name + "_input").into_boxed_str();

        sink.add_input(sink_input_id.clone());
        graph.insert_node(sink_id.clone(), sink);

        insert_success(
            &mut graph,
            (source_id.clone(), source_output_id),
            (sink_id.as_ref(), sink_input_id.as_ref()),
        )
    }

    let mut scheduler = graph.scheduler();

    for i in 0..NUM_SINKS {
        scheduler.add_sink_node(format!("sink{}", i + 1).into_boxed_str());
    }

    let schedule = scheduler.compile();

    println!("{schedule:#?}");
    println!("{:#?}", scheduler.intermediate());
}

#[test]
fn adders() {
    let mut graph = Graph::default();

    let mut sink = Node::default();
    let sink_id = Box::<str>::from("sink");
    let sink_input_id = "sink_input";
    sink.add_input(sink_input_id);
    graph.insert_node(sink_id.clone(), sink);

    let latencies = [6, 8, 13];

    for (i, lat) in latencies.into_iter().enumerate() {
        let mut source = Node::default();
        let name = format!("source{}", i + 1);
        let source_id = name.clone().into_boxed_str();
        let source_output_id = (name + "_output").into_boxed_str();

        source.add_output_with_latency(source_output_id.clone(), lat);
        graph.insert_node(source_id.clone(), source);
        insert_success(
            &mut graph,
            (source_id, source_output_id),
            (sink_id.as_ref(), sink_input_id),
        );
    }

    let mut scheduler = graph.scheduler();

    scheduler.add_sink_node(sink_id);

    let schedule = scheduler.compile();

    println!("{schedule:#?}");
    println!("{:#?}", scheduler.intermediate());
}

#[test]
fn m_graph() {
    // Picture a person doing a handstand
    let mut graph = Graph::default();

    let mut left_leg = Node::default();
    let left_leg_id = "left_leg";
    let left_foot_id = "left_foot";
    left_leg.add_output_with_latency(left_foot_id, 15);
    graph.insert_node(left_leg_id, left_leg);

    let mut right_leg = Node::default();
    let right_leg_id = "right_leg";
    let right_foot_id = "right_foot";
    right_leg.add_output_with_latency(right_foot_id, 10);
    graph.insert_node(right_leg_id, right_leg);

    let mut left_arm = Node::default();
    let left_arm_id = "left_arm";
    let left_hand_id = "left_hand";
    left_arm.add_input(left_hand_id);
    graph.insert_node(left_arm_id, left_arm);

    let mut head = Node::default();
    let head_id = "head";
    let nose_id = "nose";
    head.add_input(nose_id);
    graph.insert_node(head_id, head);

    let mut right_arm = Node::default();
    let right_arm_id = "right_arm";
    let right_hand_id = "right_hand";
    right_arm.add_input(right_hand_id);
    graph.insert_node(right_arm_id, right_arm);

    insert_success(&mut graph, (left_leg_id, left_foot_id), (head_id, nose_id));
    insert_success(
        &mut graph,
        (left_leg_id, left_foot_id),
        (left_arm_id, left_hand_id),
    );
    insert_success(
        &mut graph,
        (right_leg_id, right_foot_id),
        (head_id, nose_id),
    );
    insert_success(
        &mut graph,
        (right_leg_id, right_foot_id),
        (right_arm_id, right_hand_id),
    );

    let mut scheduler = graph.scheduler();

    scheduler.add_sink_node(left_arm_id);
    scheduler.add_sink_node(head_id);
    scheduler.add_sink_node(right_arm_id);

    let schedule = scheduler.compile();

    println!("{schedule:#?}");
    println!("{:#?}", scheduler.intermediate());
}

// basically the transpose of the m_graph
#[test]
fn w_graph() {
    // Now picture that person standing normally, and holding their hands up
    let mut graph = Graph::default();

    let mut left_arm = Node::default();
    let left_arm_id = "left_arm";
    let left_hand_id = "left_hand";
    left_arm.add_output(left_hand_id);
    graph.insert_node(left_arm_id, left_arm);

    let mut head = Node::default();
    let head_id = "head";
    let nose_id = "nose";
    head.add_output(nose_id);
    graph.insert_node(head_id, head);

    let mut right_arm = Node::default();
    let right_arm_id = "right_arm";
    let right_hand_id = "right_hand";
    right_arm.add_output(right_hand_id);
    graph.insert_node(right_arm_id, right_arm);

    let mut left_leg = Node::default();
    let left_leg_id = "left_leg";
    let left_foot_id = "left_foot";
    left_leg.add_input(left_foot_id);
    graph.insert_node(left_leg_id, left_leg);

    let mut right_leg = Node::default();
    let right_leg_id = "right_leg";
    let right_foot_id = "right_foot";
    right_leg.add_input(right_foot_id);
    graph.insert_node(right_leg_id, right_leg);

    insert_success(&mut graph, (head_id, nose_id), (left_leg_id, left_foot_id));
    insert_success(
        &mut graph,
        (left_arm_id, left_hand_id),
        (left_leg_id, left_foot_id),
    );
    insert_success(
        &mut graph,
        (head_id, nose_id),
        (right_leg_id, right_foot_id),
    );
    insert_success(
        &mut graph,
        (right_arm_id, right_hand_id),
        (right_leg_id, right_foot_id),
    );

    let mut scheduler = graph.scheduler();

    scheduler.add_sink_node(left_leg_id);
    scheduler.add_sink_node(right_leg_id);

    let schedule = scheduler.compile();

    println!("{schedule:#?}");
    println!("{:#?}", scheduler.intermediate());
}

#[test]
fn multiple_input_ports() {
    const NUM_INPUT_PORTS: usize = 4;

    let mut graph = Graph::default();

    let mut source = Node::default();
    let source_id = "source";
    let source_output_id = "source_output";
    source.add_output_with_latency(source_output_id, 13);
    graph.insert_node(source_id, source);

    let mut sink = Node::default();
    let sink_id = "sink";
    let sink_input_ids: [_; NUM_INPUT_PORTS] =
        array::from_fn(|i| format!("sink_input{}", i + 1).into_boxed_str());

    sink_input_ids
        .iter()
        .cloned()
        .for_each(|id| sink.add_input(id));
    graph.insert_node(sink_id, sink);

    for input_id in &sink_input_ids {
        insert_success(
            &mut graph,
            (source_id, source_output_id),
            (sink_id, input_id.as_ref()),
        );
    }

    let mut scheduler = graph.scheduler();

    scheduler.add_sink_node(sink_id);

    let schedule = scheduler.compile();

    println!("{schedule:#?}");
    println!("{:#?}", scheduler.intermediate());
}

#[test]
fn multiple_outputs_one_input() {
    const NUM_OUTPUT_PORTS: usize = 4;

    let mut graph = Graph::default();

    let mut source = Node::default();
    let source_id = "source";

    let source_output_ids: [_; NUM_OUTPUT_PORTS] =
        array::from_fn(|i| format!("source_output{}", i + 1).into_boxed_str());

    for (i, id) in source_output_ids.iter().cloned().enumerate() {
        source.add_output_with_latency(id, i as u64 * 4);
    }

    graph.insert_node(source_id, source);

    let mut sink = Node::default();
    let sink_id = "sink";
    let sink_input_id = "sink_input";
    sink.add_input(sink_input_id);
    graph.insert_node(sink_id, sink);

    for output_id in source_output_ids {
        insert_success(
            &mut graph,
            (source_id, output_id),
            (&sink_id, &sink_input_id),
        );
    }

    let mut scheduler = graph.scheduler();

    scheduler.add_sink_node(sink_id);

    let schedule = scheduler.compile();

    println!("{schedule:#?}");
    println!("{:#?}", scheduler.intermediate());
}
