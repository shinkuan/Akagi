async function start_mitm(){ 
    await eel.start_mitm()()  
}

async function stop_mitm(){
    await eel.stop_mitm()()
}

async function send_reach_event(a){
    await eel.send_reach_event(a)()
}

eel.expose(set_mjai_msg);
function set_mjai_msg(paragraph){
    document.getElementById("mjai_msg").innerHTML = paragraph
}

eel.expose(add_reach_button);
function add_reach_button(){
    // Add a h3 to the page
    var h3 = document.createElement("h3");
    h3.id = "reach_h3";
    h3.innerHTML = "リーチ";
    document.body.appendChild(h3);

    // Add a button to the page
    var button = document.createElement("button");
    button.id = "reach_button_yes";
    button.innerHTML = "Yes";
    button.onclick = function(){
        send_reach_event(true);
    }
    document.body.appendChild(button);

    var button = document.createElement("button");
    button.id = "reach_button_no";
    button.innerHTML = "No";
    button.onclick = function(){
        send_reach_event(false);
    }
}

eel.expose(remove_reach_button);
function remove_reach_button(){
    var element = document.getElementById("reach_h3");
    element.parentNode.removeChild(element);
    var element = document.getElementById("reach_button_yes");
    element.parentNode.removeChild(element);
    var element = document.getElementById("reach_button_no");
    element.parentNode.removeChild(element);
}